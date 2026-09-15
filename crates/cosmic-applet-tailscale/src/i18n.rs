//! Localization for the applet, following the COSMIC convention of Fluent
//! catalogues embedded at build time.

use std::sync::LazyLock;

use i18n_embed::fluent::{FluentLanguageLoader, fluent_language_loader};
use i18n_embed::{DefaultLocalizer, LanguageLoader, Localizer, unic_langid::LanguageIdentifier};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "i18n/"]
struct Localizations;

pub static LANGUAGE_LOADER: LazyLock<FluentLanguageLoader> = LazyLock::new(|| {
    let loader: FluentLanguageLoader = fluent_language_loader!();
    loader
        .load_fallback_language(&Localizations)
        .expect("the en catalogue is embedded at build time");
    loader
});

/// Apply the languages the desktop asked for.
pub fn init(requested: &[LanguageIdentifier]) {
    if let Err(error) = localizer().select(requested) {
        // A missing translation is not worth refusing to start over; the
        // fallback catalogue is always present.
        tracing::warn!(%error, "could not load the requested localizations");
    }
}

fn localizer() -> Box<dyn Localizer> {
    Box::from(DefaultLocalizer::new(&*LANGUAGE_LOADER, &Localizations))
}

/// Look up a localized string by its Fluent message ID.
#[macro_export]
macro_rules! fl {
    ($id:literal) => {{
        i18n_embed_fl::fl!($crate::i18n::LANGUAGE_LOADER, $id)
    }};
    ($id:literal, $($args:expr),*) => {{
        i18n_embed_fl::fl!($crate::i18n::LANGUAGE_LOADER, $id, $($args),*)
    }};
}
