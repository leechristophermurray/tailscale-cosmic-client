//! Background streams feeding the application.
//!
//! The daemon pushes state changes over the IPN bus, so the UI is event-driven
//! rather than poll-driven. A slow timer still runs alongside it, because the
//! bus carries prefs and backend state but not the peer map — and because it is
//! what notices that the daemon came back after a restart.

use std::sync::Arc;
use std::time::Duration;

use cosmic::iced::{Subscription, stream, window};
use futures_util::{SinkExt, StreamExt};
use tailscale_localapi::LocalApi;

use super::message::Message;

/// How long to wait before re-opening a dropped IPN bus connection. Long enough
/// not to spin while tailscaled is restarting, short enough to feel instant.
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// How often to re-read the peer map, which the bus does not push.
const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// How often to ask the monitoring hub.
///
/// Much slower than the daemon poll: the hub is another machine across the
/// network, its agents report at minute granularity anyway, and hammering
/// someone's monitoring server from a desktop client would be rude.
const HUB_INTERVAL: Duration = Duration::from_secs(60);

/// Everything the application listens to.
pub fn all() -> Subscription<Message> {
    Subscription::batch([daemon_events(), poll_timer(), hub_timer(), file_drops()])
}

/// Files dragged onto the window from `cosmic-files` (or any other file
/// manager) become a queued Taildrop transfer waiting for a target machine.
fn file_drops() -> Subscription<Message> {
    cosmic::iced::event::listen_with(|event, _status, _window| match event {
        cosmic::iced::Event::Window(window::Event::FileDropped(paths)) => {
            Some(Message::FilesDropped(Arc::new(paths)))
        }
        _ => None,
    })
}

/// A self-healing subscription to the IPN bus.
fn daemon_events() -> Subscription<Message> {
    Subscription::run(|| {
        stream::channel(16, async |mut output| {
            let api = LocalApi::default();

            loop {
                match api.watch().await {
                    Ok(notifications) => {
                        let mut notifications = Box::pin(notifications);

                        while let Some(frame) = notifications.next().await {
                            match frame {
                                Ok(notify) if notify.is_meaningful() => {
                                    if output
                                        .send(Message::DaemonEvent(Arc::new(notify)))
                                        .await
                                        .is_err()
                                    {
                                        // The application is gone; stop.
                                        return;
                                    }
                                }
                                Ok(_) => {}
                                Err(error) => {
                                    tracing::debug!(%error, "IPN bus frame dropped");
                                    break;
                                }
                            }
                        }
                    }
                    Err(error) => {
                        tracing::debug!(%error, "could not attach to the IPN bus");
                    }
                }

                // The stream ended or never started. Tell the app so it can
                // re-read state, then back off and try again.
                if output.send(Message::DaemonStreamEnded).await.is_err() {
                    return;
                }

                tokio::time::sleep(RECONNECT_DELAY).await;
            }
        })
    })
}

/// A slow heartbeat that refreshes the peer map.
fn poll_timer() -> Subscription<Message> {
    cosmic::iced::time::every(POLL_INTERVAL).map(|_| Message::Tick)
}

/// A slower heartbeat for the monitoring hub.
fn hub_timer() -> Subscription<Message> {
    cosmic::iced::time::every(HUB_INTERVAL).map(|_| Message::BeszelRefresh)
}
