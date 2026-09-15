//! Desktop notifications.
//!
//! These go through the freedesktop notification service, which on COSMIC is
//! `cosmic-notifications`. Notifications are fire-and-forget from the update
//! loop's point of view: a notification daemon that is missing or slow must
//! never stall the UI, so every call is spawned onto a blocking task and its
//! failures are logged rather than surfaced.

use std::path::PathBuf;

use notify_rust::{Notification, Timeout};
use tailscale_localapi::{LocalApi, WaitingFile};

use super::message::Message;
use crate::fl;

/// Matches the desktop entry, so the notification carries the app's icon and
/// the shell can group it with the application.
const APP_ID: &str = "com.system76.CosmicTailscale";
const ICON: &str = "com.system76.CosmicTailscale-symbolic";

/// Where received files are written.
fn download_dir() -> PathBuf {
    dirs::download_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Downloads")
    })
}

/// Announce files that have arrived over Taildrop, offering to save them.
///
/// The daemon holds received files in a queue until something claims them, so
/// the "Save" action is what actually writes them to disk.
pub fn taildrop_arrived(files: &[WaitingFile]) -> cosmic::app::Task<Message> {
    if files.is_empty() {
        return cosmic::app::Task::none();
    }

    let count = files.len();
    let summary = if count == 1 {
        fl!("notif-taildrop-one")
    } else {
        fl!("notif-taildrop-many", count = count)
    };

    let body = files
        .iter()
        .map(|file| format!("{} ({})", file.name, file.human_size()))
        .collect::<Vec<_>>()
        .join("\n");

    let names: Vec<String> = files.iter().map(|file| file.name.clone()).collect();

    cosmic::task::future(async move {
        let action = tokio::task::spawn_blocking(move || {
            Notification::new()
                .appname("Tailscale")
                .summary(&summary)
                .body(&body)
                .icon(ICON)
                .hint(notify_rust::Hint::DesktopEntry(APP_ID.to_string()))
                .action("save", &fl!("notif-save-downloads"))
                .action("show", &fl!("notif-show-app"))
                .timeout(Timeout::Never)
                .show()
                .map(|handle| {
                    let mut chosen = None;
                    handle.wait_for_action(|action| chosen = Some(action.to_string()));
                    chosen
                })
        })
        .await;

        match action {
            Ok(Ok(Some(action))) if action == "save" => Message::SaveWaitingFiles(names),
            // "show" just needs the window, which is already what we are.
            _ => Message::Noop,
        }
    })
}

/// Warn that this machine's node key is about to expire.
///
/// Tailscale signs a node out when its key expires, so this is the one
/// notification worth interrupting someone for.
pub fn key_expiring(days: i64) -> cosmic::app::Task<Message> {
    let body = if days <= 0 {
        fl!("notif-key-expired")
    } else if days == 1 {
        fl!("notif-key-tomorrow")
    } else {
        fl!("notif-key-days", days = days)
    };

    cosmic::task::future(async move {
        let action = tokio::task::spawn_blocking(move || {
            Notification::new()
                .appname("Tailscale")
                .summary(&fl!("notif-key-title"))
                .body(&body)
                .icon(ICON)
                .hint(notify_rust::Hint::DesktopEntry(APP_ID.to_string()))
                .action("reauth", &fl!("reauthenticate"))
                .timeout(Timeout::Never)
                .show()
                .map(|handle| {
                    let mut chosen = None;
                    handle.wait_for_action(|action| chosen = Some(action.to_string()));
                    chosen
                })
        })
        .await;

        match action {
            Ok(Ok(Some(action))) if action == "reauth" => Message::LoginRequested,
            _ => Message::Noop,
        }
    })
}

/// Report hardware problems the monitoring hub is flagging.
///
/// One notification covering every affected machine, rather than one each: a
/// power cut that takes out four servers should not produce four popups.
pub fn hardware_warning(warnings: &[String]) -> cosmic::app::Task<Message> {
    if warnings.is_empty() {
        return cosmic::app::Task::none();
    }

    let body = warnings.join("\n");

    cosmic::task::future(async move {
        let _ = tokio::task::spawn_blocking(move || {
            Notification::new()
                .appname("Tailscale")
                .summary(&fl!("notif-hardware-title"))
                .body(&body)
                .icon(ICON)
                .hint(notify_rust::Hint::DesktopEntry(APP_ID.to_string()))
                .timeout(Timeout::Never)
                .show()
        })
        .await;

        Message::Noop
    })
}

/// Write queued Taildrop files into the downloads folder.
///
/// Each file is acknowledged only after it lands on disk, so a failure leaves
/// it in the daemon's queue to retry rather than dropping it.
pub fn save_waiting_files(api: &LocalApi, names: Vec<String>) -> cosmic::app::Task<Message> {
    let api = api.clone();

    cosmic::task::future(async move {
        let dir = download_dir();

        if let Err(error) = tokio::fs::create_dir_all(&dir).await {
            return Message::TaildropCompleted(Err(super::message::Failure {
                message: format!("Could not create {}: {error}", dir.display()),
                unreachable: false,
            }));
        }

        let mut saved = 0usize;

        for name in &names {
            let contents = match api.fetch_file(name).await {
                Ok(contents) => contents,
                Err(error) => {
                    return Message::TaildropCompleted(Err(super::message::Failure {
                        message: format!("Could not receive {name}: {error}"),
                        unreachable: error.is_unreachable(),
                    }));
                }
            };

            let destination = unique_path(&dir, name);

            if let Err(error) = tokio::fs::write(&destination, &contents).await {
                return Message::TaildropCompleted(Err(super::message::Failure {
                    message: format!("Could not write {}: {error}", destination.display()),
                    unreachable: false,
                }));
            }

            // Only now is it safe to let the daemon forget the transfer.
            if let Err(error) = api.acknowledge_file(name).await {
                tracing::warn!(%name, %error, "saved the file but could not clear it from the queue");
            }

            saved += 1;
        }

        Message::TaildropCompleted(Ok(fl!(
            "taildrop-saved",
            files = crate::ui::file_count(saved),
            path = dir.display().to_string()
        )))
    })
}

/// Avoid silently overwriting an existing download by suffixing the stem.
fn unique_path(dir: &std::path::Path, name: &str) -> PathBuf {
    // Strip any directory components a peer may have put in the name; a
    // received filename must never escape the downloads folder.
    let name = std::path::Path::new(name)
        .file_name()
        .map_or_else(|| "taildrop-file".to_string(), |n| n.to_string_lossy().into_owned());

    let candidate = dir.join(&name);
    if !candidate.exists() {
        return candidate;
    }

    let path = std::path::Path::new(&name);
    let stem = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let extension = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();

    for index in 1..1000 {
        let candidate = dir.join(format!("{stem} ({index}){extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }

    candidate
}
