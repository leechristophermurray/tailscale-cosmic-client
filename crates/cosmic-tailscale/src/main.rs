//! `cosmic-tailscale` — a native Tailscale client for the COSMIC desktop.
//!
//! The window talks to the local `tailscaled` over its UNIX socket and renders
//! the result with `libcosmic`, so it inherits the desktop's theme, accent
//! colour, typography, and density with no configuration of its own.

mod app;
mod i18n;
mod pages;
mod ui;

/// Files named on the command line, which is how the file manager hands a
/// "Send via Taildrop" selection to us.
///
/// Anything that is not an existing path is ignored rather than rejected: the
/// desktop entry uses `%F`, and a stale selection should not stop the window
/// from opening.
fn files_from_args() -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    for arg in std::env::args_os().skip(1) {
        if arg == "--send" {
            continue;
        }
        let path = std::path::PathBuf::from(arg);
        if path.is_file() {
            files.push(path);
        }
    }

    files
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cosmic_tailscale=info,warn".into()),
        )
        .init();

    i18n::init(&i18n_embed::DesktopLanguageRequester::requested_languages());

    let settings = cosmic::app::Settings::default()
        .size(cosmic::iced::Size::new(1280.0, 860.0))
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(760.0)
                .min_height(560.0),
        );

    cosmic::app::run::<app::App>(
        settings,
        app::Flags {
            pending_files: files_from_args(),
        },
    )?;
    Ok(())
}
