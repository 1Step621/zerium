//! Application translations and locale selection.

use ::ui as components;

pub(crate) fn initialize(language: Option<&str>) {
    rust_i18n::extend!(components);
    let locale = language
        .map(str::trim)
        .filter(|language| !language.is_empty())
        .map(str::to_owned)
        .or_else(sys_locale::get_locale)
        .unwrap_or_else(|| "en-us".to_owned());
    rust_i18n::set_locale(&locale.replace('_', "-").to_ascii_lowercase());
}

pub(crate) fn locale() -> String {
    rust_i18n::locale().to_string()
}
