//! UTF-8 validation and parsing helpers that abstract over [`simdutf8`] if the
//! `simd` feature is enabled, otherwise fall back to [`std`] equivalents.
use crate::proto::ProtocolError;

/// Attempts to parse an input of bytes as a UTF-8 string with SIMD
/// acceleration.
///
/// # Errors
///
/// This method returns a [`ProtocolError`] if the input is invalid UTF-8.
#[cfg(feature = "simd")]
#[inline]
pub fn parse_str(input: &[u8]) -> Result<&str, ProtocolError> {
    match simdutf8::basic::from_utf8(input) {
        Ok(string) => Ok(string),
        Err(_) => Err(ProtocolError::InvalidUtf8),
    }
}

/// Attempts to parse an input of bytes as a UTF-8 string.
///
/// # Errors
///
/// This method returns a [`ProtocolError`] if the input is invalid UTF-8.
#[cfg(not(feature = "simd"))]
#[inline]
pub fn parse_str(input: &[u8]) -> Result<&str, ProtocolError> {
    Ok(std::str::from_utf8(input)?)
}

/// Attempts to parse an input of bytes as a partial UTF-8 string with SIMD
/// acceleration.
///
/// The return type is a tuple with a boolean indicating whether the input is
/// invalid UTF-8 up to this point and the index of the last valid UTF-8
/// codepoint byte.
#[cfg(feature = "simd")]
#[inline]
pub fn should_fail_fast(input: &[u8], is_complete: bool) -> (bool, usize) {
    // We only need the extra info about the valid codepoints if the frame is
    // incomplete
    if is_complete {
        if simdutf8::basic::from_utf8(input).is_ok() {
            (false, input.len())
        } else {
            (true, 0)
        }
    } else {
        match simdutf8::compat::from_utf8(input) {
            Ok(_) => (false, input.len()),
            Err(utf8_error) => (utf8_error.error_len().is_some(), utf8_error.valid_up_to()),
        }
    }
}

/// Attempts to parse an input of bytes as a partial UTF-8 string.
///
/// The return type is a tuple with a boolean indicating whether the input is
/// invalid UTF-8 up to this point and the index of the last valid UTF-8
/// codepoint byte.
#[cfg(not(feature = "simd"))]
#[inline]
pub fn should_fail_fast(input: &[u8], is_complete: bool) -> (bool, usize) {
    match std::str::from_utf8(input) {
        Ok(_) => (false, input.len()),
        Err(utf8_error) => {
            if is_complete {
                (true, 0)
            } else {
                (utf8_error.error_len().is_some(), utf8_error.valid_up_to())
            }
        }
    }
}
