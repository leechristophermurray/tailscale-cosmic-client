//! A stand-in HTTP/1.1 server for tests.
//!
//! The service clients in this workspace have each had a bug that lived in the
//! exact bytes on the wire — an absolute-form request line that Caddy's Go
//! server read the Host from, a notify mask one bit off in a query string. A
//! mock at the function level cannot see those. This records every request
//! *as written*, request line and headers verbatim, and answers with whatever
//! the test scripts.
//!
//! It is deliberately small: one request per connection, always
//! `Connection: close`, so there is no keep-alive state to get wrong.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// A request exactly as the client sent it.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// The request-target verbatim: `/config/` in origin-form, or
    /// `http://host/config/` in absolute-form. Tests assert on this.
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// Header value by case-insensitive name.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The path, without the query string.
    #[must_use]
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or(&self.target)
    }

    /// The raw query string, if any.
    #[must_use]
    pub fn query(&self) -> Option<&str> {
        self.target.split_once('?').map(|(_, query)| query)
    }

    #[must_use]
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// How to answer a request.
pub enum Reply {
    /// A complete response with a known length.
    Full {
        status: u16,
        content_type: &'static str,
        body: Vec<u8>,
    },
    /// Headers, then body pieces written with a pause after each, then close.
    /// The pauses make each piece arrive as its own read, which is how a
    /// client's line reassembly across chunk boundaries gets exercised.
    Stream {
        status: u16,
        pieces: Vec<Vec<u8>>,
        pause: Duration,
    },
}

impl Reply {
    #[must_use]
    pub fn json(status: u16, body: impl Into<String>) -> Self {
        Self::Full {
            status,
            content_type: "application/json",
            body: body.into().into_bytes(),
        }
    }

    #[must_use]
    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self::Full {
            status,
            content_type: "text/plain",
            body: body.into().into_bytes(),
        }
    }

    #[must_use]
    pub fn empty(status: u16) -> Self {
        Self::Full {
            status,
            content_type: "text/plain",
            body: Vec::new(),
        }
    }
}

type Handler = Arc<dyn Fn(&Request) -> Reply + Send + Sync>;

/// A running stand-in server. Dropping it stops accepting connections.
pub struct Stub {
    requests: Arc<Mutex<Vec<Request>>>,
    address: Address,
    task: tokio::task::JoinHandle<()>,
}

enum Address {
    Tcp(std::net::SocketAddr),
    Unix(PathBuf),
}

impl Stub {
    /// Listen on an ephemeral loopback TCP port.
    pub async fn tcp(handler: impl Fn(&Request) -> Reply + Send + Sync + 'static) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let address = listener.local_addr().expect("local address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler: Handler = Arc::new(handler);

        let recorded = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(serve(stream, Arc::clone(&handler), Arc::clone(&recorded)));
            }
        });

        Self {
            requests,
            address: Address::Tcp(address),
            task,
        }
    }

    /// Listen on a UNIX socket at `path`, as tailscaled does.
    pub async fn unix(
        path: impl AsRef<Path>,
        handler: impl Fn(&Request) -> Reply + Send + Sync + 'static,
    ) -> Self {
        let path = path.as_ref().to_path_buf();
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path).expect("bind unix socket");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler: Handler = Arc::new(handler);

        let recorded = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(serve(stream, Arc::clone(&handler), Arc::clone(&recorded)));
            }
        });

        Self {
            requests,
            address: Address::Unix(path),
            task,
        }
    }

    /// `http://127.0.0.1:<port>` for a TCP stub.
    #[must_use]
    pub fn url(&self) -> String {
        match &self.address {
            Address::Tcp(address) => format!("http://{address}"),
            Address::Unix(path) => format!("unix://{}", path.display()),
        }
    }

    #[must_use]
    pub fn port(&self) -> u16 {
        match &self.address {
            Address::Tcp(address) => address.port(),
            Address::Unix(_) => 0,
        }
    }

    /// Every request received so far, in arrival order.
    #[must_use]
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().expect("requests lock").clone()
    }

    /// The only request received. Panics if there was not exactly one, which
    /// is itself worth a test failing over.
    #[must_use]
    pub fn only_request(&self) -> Request {
        let requests = self.requests();
        assert_eq!(
            requests.len(),
            1,
            "expected exactly one request, got {requests:#?}"
        );
        requests.into_iter().next().expect("one request")
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.task.abort();
        if let Address::Unix(path) = &self.address {
            let _ = std::fs::remove_file(path);
        }
    }
}

async fn serve<S>(mut stream: S, handler: Handler, recorded: Arc<Mutex<Vec<Request>>>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let Some(request) = read_request(&mut stream).await else {
        return;
    };

    let reply = handler(&request);
    recorded.lock().expect("requests lock").push(request);

    match reply {
        Reply::Full {
            status,
            content_type,
            body,
        } => {
            let head = format!(
                "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                reason(status),
                body.len()
            );
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(&body).await;
        }
        Reply::Stream {
            status,
            pieces,
            pause,
        } => {
            // No Content-Length: the body runs until the connection closes.
            let head = format!(
                "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
                reason(status)
            );
            let _ = stream.write_all(head.as_bytes()).await;
            for piece in pieces {
                let _ = stream.write_all(&piece).await;
                let _ = stream.flush().await;
                tokio::time::sleep(pause).await;
            }
        }
    }

    let _ = stream.flush().await;
    let _ = stream.shutdown().await;
}

async fn read_request<S: AsyncRead + Unpin>(stream: &mut S) -> Option<Request> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];

    // Read until the end of the head.
    let head_end = loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = find(&buffer, b"\r\n\r\n") {
            break position;
        }
    };

    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_string();
    let target = request_line.next()?.to_string();

    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect();

    let length = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);

    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);

    Some(Request {
        method,
        target,
        headers,
        body,
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    }
}
