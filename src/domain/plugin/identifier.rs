use super::PluginError;

/// Validates an identifier used only for manifest lookup and namespacing.
pub(super) fn validate_logical_id(kind: &str, id: &str) -> Result<(), PluginError> {
    let mut bytes = id.bytes();
    let valid = bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    valid.then_some(()).ok_or_else(|| {
        PluginError::invalid_definition(format!(
            "{kind} ID '{id}' must start with an ASCII letter or digit and contain only ASCII letters, digits, '.', '_', or '-'"
        ))
    })
}

/// Validates a complete identifier emitted directly into generated WGSL.
pub(super) fn validate_wgsl_identifier(kind: &str, id: &str) -> Result<(), PluginError> {
    let mut bytes = id.bytes();
    let valid_syntax = bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric());
    if !valid_syntax {
        return Err(PluginError::invalid_definition(format!(
            "{kind} ID '{id}' must be an ASCII WGSL identifier"
        )));
    }
    if id == "_" || id.starts_with("__") || naga::keywords::wgsl::RESERVED.contains(&id) {
        return Err(PluginError::invalid_definition(format!(
            "{kind} ID '{id}' is reserved by WGSL"
        )));
    }
    Ok(())
}

/// Validates a fragment appended after a safe prefix in generated WGSL.
///
/// Unlike a complete identifier, a suffix may start with a digit or contain a
/// reserved word because the generated identifier starts with `media_`.
pub(super) fn validate_wgsl_identifier_suffix(kind: &str, id: &str) -> Result<(), PluginError> {
    let valid = !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric());
    valid.then_some(()).ok_or_else(|| {
        PluginError::invalid_definition(format!(
            "{kind} ID '{id}' must contain only ASCII letters, digits, or '_'"
        ))
    })
}
