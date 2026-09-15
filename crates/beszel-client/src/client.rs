//! A client for a Beszel hub's PocketBase API.
//!
//! The hub is reached over the tailnet, so no part of it needs to be on the
//! public internet. Beszel stores everything in PocketBase, whose REST API is
//! what this speaks: authenticate once for a token, then read collections.

use std::time::Duration;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::model::{
    ContainerStats, ListResponse, Stats, StatsPeriod, SystemRecord,
    container::ContainerStatsRecord, stats::StatsRecord,
};

/// Hub requests are all small reads; a hub that has not answered in this long
/// is down rather than slow.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// PocketBase's own cap on page size.
const MAX_PAGE: u32 = 500;

/// An authenticated session with a hub.
#[derive(Debug, Clone)]
pub struct BeszelHub {
    base: String,
    http: reqwest::Client,
    token: Option<String>,
}

/// What a successful sign-in returns.
#[derive(Debug, Clone, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    #[serde(default)]
    pub record: AuthRecord,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct AuthRecord {
    pub id: String,
    pub email: String,
}

/// What `/api/beszel/info` reports.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct HubInfo {
    /// The hub's SSH public key, which an agent is installed with.
    pub key: String,
    /// Hub version. Beszel abbreviates this one too.
    #[serde(rename = "v")]
    pub version: String,
}

impl BeszelHub {
    /// Point at a hub, e.g. `https://mon.example.com` or
    /// `http://monitor-node.tailnet.ts.net:8090`.
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let base = base_url.into();
        let base = base.trim_end_matches('/').to_string();

        if !base.starts_with("http://") && !base.starts_with("https://") {
            return Err(Error::BadUrl {
                url: base,
                reason: "expected an http:// or https:// address".to_string(),
            });
        }

        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("cosmic-tailscale/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|source| Error::Unreachable {
                url: base.clone(),
                source,
            })?;

        Ok(Self {
            base,
            http,
            token: None,
        })
    }

    /// Resume a session with a token from a previous sign-in.
    #[must_use]
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base
    }

    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.token.is_some()
    }

    /// Check the hub is there, without needing credentials.
    ///
    /// PocketBase answers `/api/health` unauthenticated, which separates "wrong
    /// address" from "wrong password" before the user has typed one.
    pub async fn health(&self) -> Result<()> {
        let url = format!("{}/api/health", self.base);

        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|source| Error::Unreachable {
                url: url.clone(),
                source,
            })?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(Error::Status {
                status: response.status(),
                message: "the address answered, but not like a Beszel hub".to_string(),
            })
        }
    }

    /// Sign in and keep the token for subsequent calls.
    pub async fn authenticate(&mut self, identity: &str, password: &str) -> Result<AuthResponse> {
        let url = format!("{}/api/collections/users/auth-with-password", self.base);

        let response = self
            .http
            .post(&url)
            .json(&serde_json::json!({
                "identity": identity,
                "password": password,
            }))
            .send()
            .await
            .map_err(|source| Error::Unreachable {
                url: url.clone(),
                source,
            })?;

        if response.status() == reqwest::StatusCode::BAD_REQUEST
            || response.status() == reqwest::StatusCode::UNAUTHORIZED
        {
            // PocketBase answers 400 for bad credentials, which would otherwise
            // surface as an unhelpful "bad request".
            return Err(Error::Unauthorized);
        }

        let response = check_status(response).await?;
        let auth: AuthResponse = response.json().await.map_err(Error::Decode)?;

        self.token = Some(auth.token.clone());
        Ok(auth)
    }

    /// Forget the token. Does not revoke it on the hub.
    pub fn sign_out(&mut self) {
        self.token = None;
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let Some(token) = &self.token else {
            return Err(Error::NotAuthenticated);
        };

        let url = format!("{}{path}", self.base);

        let response = self
            .http
            .get(&url)
            .header(reqwest::header::AUTHORIZATION, token)
            .send()
            .await
            .map_err(|source| Error::Unreachable {
                url: url.clone(),
                source,
            })?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::Unauthorized);
        }

        let response = check_status(response).await?;
        response.json().await.map_err(Error::Decode)
    }

    /// The hub's public key and version.
    ///
    /// The key is what a new agent authenticates the hub with, so this is the
    /// first step of enrolling a machine.
    pub async fn info(&self) -> Result<HubInfo> {
        self.get_json("/api/beszel/info").await
    }

    /// Every machine the hub monitors.
    pub async fn systems(&self) -> Result<Vec<SystemRecord>> {
        let path = format!("/api/collections/systems/records?perPage={MAX_PAGE}&sort=name");
        let list: ListResponse<SystemRecord> = self.get_json(&path).await?;
        Ok(list.items)
    }

    /// The most recent stats sample for one machine.
    pub async fn latest_stats(&self, system_id: &str) -> Result<Option<Stats>> {
        let records = self.stats(system_id, StatsPeriod::OneMinute, 1).await?;
        Ok(records.into_iter().next().map(|record| record.stats))
    }

    /// A stats series for one machine, newest first.
    pub async fn stats(
        &self,
        system_id: &str,
        period: StatsPeriod,
        limit: u32,
    ) -> Result<Vec<StatsRecord>> {
        let filter = urlencode(&format!(
            "system='{}' && type='{}'",
            escape_filter(system_id),
            period.as_str()
        ));

        let path = format!(
            "/api/collections/system_stats/records?filter={filter}&sort=-created&perPage={}",
            limit.min(MAX_PAGE)
        );

        let list: ListResponse<StatsRecord> = self.get_json(&path).await?;
        Ok(list.items)
    }

    /// The containers running on one machine, from its latest sample.
    pub async fn containers(&self, system_id: &str) -> Result<Vec<ContainerStats>> {
        let filter = urlencode(&format!(
            "system='{}' && type='{}'",
            escape_filter(system_id),
            StatsPeriod::OneMinute.as_str()
        ));

        let path = format!(
            "/api/collections/container_stats/records?filter={filter}&sort=-created&perPage=1"
        );

        let list: ListResponse<ContainerStatsRecord> = self.get_json(&path).await?;

        let mut containers = list
            .items
            .into_iter()
            .next()
            .map(|record| record.stats)
            .unwrap_or_default();

        containers.sort_by(|a, b| {
            b.cpu
                .partial_cmp(&a.cpu)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(containers)
    }
}

/// Turn a non-success response into an error carrying PocketBase's own message,
/// which says which field it objected to.
#[derive(Deserialize)]
struct PocketBaseError {
    #[serde(default)]
    message: String,
}

async fn check_status(response: reqwest::Response) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let message = response
        .json::<PocketBaseError>()
        .await
        .map(|body| body.message)
        .unwrap_or_default();

    Err(Error::Status {
        status,
        message: if message.is_empty() {
            "no detail given".to_string()
        } else {
            message
        },
    })
}

/// Escape a value being interpolated into a PocketBase filter expression.
///
/// Record ids are hub-generated and alphanumeric, but building a query by
/// string concatenation without escaping is a habit worth not forming.
fn escape_filter(value: &str) -> String {
    value.replace('\\', r"\\").replace('\'', r"\'")
}

/// Percent-encode a query parameter value.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}
