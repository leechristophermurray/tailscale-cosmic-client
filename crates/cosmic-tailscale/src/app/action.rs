//! Async work, expressed as tasks that resolve into [`Message`]s.
//!
//! Nothing here touches the UI state directly: every call returns a task whose
//! result flows back through `update`, so the daemon is always the source of
//! truth and an in-flight write can never leave the view inconsistent.

use std::sync::Arc;

use cosmic::app::Task;
use tailscale_localapi::{LocalApi, MaskedPrefs, PingType};

use super::message::{Failure, Message};
use crate::fl;

/// Read the whole world: status, prefs, published services, and Taildrop state.
pub fn refresh_all(api: &LocalApi) -> Task<Message> {
    cosmic::task::batch(vec![
        status(api),
        prefs(api),
        serve_config(api),
        file_targets(api),
        waiting_files(api),
    ])
}

pub fn status(api: &LocalApi) -> Task<Message> {
    let api = api.clone();
    cosmic::task::future(async move {
        Message::StatusLoaded(api.status().await.map(Arc::new).map_err(Failure::from))
    })
}

pub fn prefs(api: &LocalApi) -> Task<Message> {
    let api = api.clone();
    cosmic::task::future(async move {
        Message::PrefsLoaded(api.prefs().await.map(Arc::new).map_err(Failure::from))
    })
}

pub fn serve_config(api: &LocalApi) -> Task<Message> {
    let api = api.clone();
    cosmic::task::future(async move {
        Message::ServeLoaded(
            api.serve_config()
                .await
                .map(Arc::new)
                .map_err(Failure::from),
        )
    })
}

pub fn file_targets(api: &LocalApi) -> Task<Message> {
    let api = api.clone();
    cosmic::task::future(async move {
        Message::FileTargetsLoaded(
            api.file_targets()
                .await
                .map(Arc::new)
                .map_err(Failure::from),
        )
    })
}

pub fn waiting_files(api: &LocalApi) -> Task<Message> {
    let api = api.clone();
    cosmic::task::future(async move {
        Message::WaitingFilesLoaded(
            api.waiting_files()
                .await
                .map(Arc::new)
                .map_err(Failure::from),
        )
    })
}

/// Apply a partial prefs change. The daemon's resulting prefs come back as
/// `PrefsApplied` so the toggles reconcile against reality, not against what we
/// optimistically assumed.
pub fn apply_prefs(api: &LocalApi, prefs: MaskedPrefs) -> Task<Message> {
    let api = api.clone();
    cosmic::task::future(async move {
        Message::PrefsApplied(
            api.set_prefs(prefs)
                .await
                .map(Arc::new)
                .map_err(Failure::from),
        )
    })
}

pub fn login(api: &LocalApi) -> Task<Message> {
    let api = api.clone();
    cosmic::task::future(async move {
        match api.login_interactive().await {
            // The daemon pushes the browser URL over the IPN bus once the login
            // flow starts, so there is nothing to report here on success.
            Ok(()) => Message::Noop,
            Err(error) => Message::PrefsApplied(Err(Failure::from(error))),
        }
    })
}

/// Probe a peer over the real WireGuard path.
pub fn ping(api: &LocalApi, stable_id: String, ip: String) -> Task<Message> {
    let api = api.clone();
    cosmic::task::future(async move {
        let result = api
            .ping(&ip, PingType::Disco)
            .await
            .map(Arc::new)
            .map_err(Failure::from);
        Message::PingCompleted(stable_id, result)
    })
}

/// Push one or more files to a peer over Taildrop.
///
/// Each file is read and sent in turn; the first failure stops the batch, since
/// continuing after a rejected transfer usually just repeats the same error.
pub fn send_files(
    api: &LocalApi,
    stable_id: String,
    peer_name: String,
    paths: Vec<std::path::PathBuf>,
) -> Task<Message> {
    let api = api.clone();
    cosmic::task::future(async move {
        let count = paths.len();

        for path in paths {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".to_string());

            let contents = match tokio::fs::read(&path).await {
                Ok(contents) => contents,
                Err(error) => {
                    return Message::TaildropCompleted(Err(Failure {
                        message: format!("Could not read {name}: {error}"),
                        unreachable: false,
                    }));
                }
            };

            if let Err(error) = api.send_file(&stable_id, &name, contents).await {
                return Message::TaildropCompleted(Err(Failure::from(error)));
            }
        }

        Message::TaildropCompleted(Ok(fl!(
            "taildrop-sent",
            files = crate::ui::file_count(count),
            name = peer_name.as_str()
        )))
    })
}

/// Ask the desktop's file chooser for files to send.
///
/// This goes through the XDG portal rather than drawing a picker in-process, so
/// it is the same dialog every other application uses and it honours the
/// sandbox's file access rules.
pub fn choose_files(target: Option<String>) -> Task<Message> {
    cosmic::task::future(async move {
        let dialog = cosmic::dialog::file_chooser::open::Dialog::new()
            .title(fl!("choose-files-title"))
            .accept_label(fl!("choose-files-accept"));

        match dialog.open_files().await {
            Ok(response) => {
                // The portal hands back URLs; anything that is not a local file
                // is not something Taildrop can read.
                let paths: Vec<std::path::PathBuf> = response
                    .urls()
                    .iter()
                    .filter(|url| url.scheme() == "file")
                    .filter_map(|url| url.to_file_path().ok())
                    .collect();

                if paths.is_empty() {
                    Message::FileChooserClosed
                } else {
                    Message::FilesChosen(target, Arc::new(paths))
                }
            }

            Err(cosmic::dialog::file_chooser::Error::Cancelled) => Message::FileChooserClosed,

            Err(error) => Message::TaildropCompleted(Err(Failure {
                message: format!("Could not open the file chooser: {error}"),
                unreachable: false,
            })),
        }
    })
}

/// Open a Tailscale SSH session in the COSMIC terminal.
///
/// `tailscale ssh` handles authentication from the node's tailnet identity, so
/// no key material or username prompt is involved.
pub fn ssh_to(peer: String) -> Task<Message> {
    cosmic::task::future(async move {
        let result = tokio::process::Command::new("cosmic-term")
            .arg("--")
            .arg("tailscale")
            .arg("ssh")
            .arg(&peer)
            .spawn();

        match result {
            Ok(_) => Message::Noop,
            Err(error) => Message::TaildropCompleted(Err(Failure {
                message: format!("Could not launch cosmic-term: {error}"),
                unreachable: false,
            })),
        }
    })
}

/// Hand a URL to the desktop's default handler.
pub fn open_url(url: String) -> Task<Message> {
    cosmic::task::future(async move {
        if let Err(error) = open::that_detached(&url) {
            return Message::TaildropCompleted(Err(Failure {
                message: format!("Could not open {url}: {error}"),
                unreachable: false,
            }));
        }
        Message::Noop
    })
}

/// Bring the tunnel back up after a suspend window elapses.
pub fn resume_after(duration: std::time::Duration) -> Task<Message> {
    cosmic::task::future(async move {
        tokio::time::sleep(duration).await;
        Message::SuspendElapsed
    })
}

// ---- remote files ----------------------------------------------------------------

/// Read which SFTP locations GVfs has mounted.
pub fn list_mounts() -> Task<Message> {
    cosmic::task::future(async move {
        let result = super::mounts::list(&super::mounts::Tools::default()).await;
        Message::MountsLoaded(result.map(Arc::new))
    })
}

/// Mount a machine, then optionally open it in Files.
pub fn mount(peer_id: String, remote: super::mounts::Remote, open: bool) -> Task<Message> {
    cosmic::task::future(async move {
        let result = super::mounts::mount(&super::mounts::Tools::default(), &remote).await;
        Message::MountFinished(peer_id, open, result)
    })
}

pub fn unmount(peer_id: String, remote: super::mounts::Remote) -> Task<Message> {
    cosmic::task::future(async move {
        let result = super::mounts::unmount(&super::mounts::Tools::default(), &remote).await;
        Message::UnmountFinished(peer_id, result)
    })
}

/// Open a location in COSMIC Files.
pub fn open_in_files(location: String) -> Task<Message> {
    cosmic::task::future(async move {
        match super::mounts::open_in_files(&super::mounts::Tools::default(), &location).await {
            Ok(()) => Message::Noop,
            Err(message) => Message::TaildropCompleted(Err(Failure {
                message,
                unreachable: false,
            })),
        }
    })
}
