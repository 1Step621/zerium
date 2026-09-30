//! Application translations and system locale selection.

pub(crate) fn initialize() {
    let locale = sys_locale::get_locale()
        .map(|locale| locale.to_lowercase())
        .unwrap_or_else(|| "en-us".to_owned());
    rust_i18n::set_locale(&locale);
}

pub(crate) fn locale() -> String {
    rust_i18n::locale().to_string()
}
