//! Application translations and locale selection.

pub(crate) fn initialize() {
    let locale = std::env::var("ZERIUM_LANGUAGE")
        .ok()
        .or_else(sys_locale::get_locale)
        .unwrap_or_else(|| "en-us".to_owned());
    rust_i18n::set_locale(&locale.to_ascii_lowercase());
}

pub(crate) fn locale() -> String {
    rust_i18n::locale().to_string()
}
