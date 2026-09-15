//! `cosmic-applet-tailscale` — Tailscale in the COSMIC panel.

mod applet;
mod i18n;

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cosmic_applet_tailscale=info,warn".into()),
        )
        .init();

    i18n::init(&i18n_embed::DesktopLanguageRequester::requested_languages());

    cosmic::applet::run::<applet::Applet>(())
}
