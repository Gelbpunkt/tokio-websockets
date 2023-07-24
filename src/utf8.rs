//! UTF-8 validation and parsing helpers that abstract over [`simdutf8`] if the
//! `simd` feature is enabled, otherwise fall back to [`std`] equivalents.
use crate::proto::ProtocolError;

/// Converts a slice of bytes to a string slice with SIMD acceleration.
///
/// # Errors
///
/// Returns a [`ProtocolError`] if the input is invalid UTF-8.
#[cfg(feature = "simd")]
#[inline]
pub fn parse_str(input: &[u8]) -> Result<&str, ProtocolError> {
    simdutf8::basic::from_utf8(input).map_err(|_| ProtocolError::InvalidUtf8)
}

/// Converts a slice of bytes to a string slice.
///
/// # Errors
///
/// Returns a [`ProtocolError`] if the input is invalid UTF-8.
#[cfg(not(feature = "simd"))]
#[inline]
pub fn parse_str(input: &[u8]) -> Result<&str, ProtocolError> {
    std::str::from_utf8(input).map_err(|_| ProtocolError::InvalidUtf8)
}

/// Retrieve the index of the last valid UTF-8 codepoint with SIMD acceleration.
///
/// # Errors
///
/// Returns [`ProtocolError`] if the input is invalid UTF-8 or if the input is
/// complete but did ended on a partial codepoint.
#[cfg(feature = "simd")]
#[inline]
pub fn should_fail_fast(input: &[u8], is_complete: bool) -> Result<usize, ProtocolError> {
    // We only need the extra info about the valid codepoints if the frame is
    // incomplete
    if is_complete {
        if simdutf8::basic::from_utf8(input).is_ok() {
            Ok(input.len())
        } else {
            Err(ProtocolError::InvalidUtf8)
        }
    } else {
        match simdutf8::compat::from_utf8(input) {
            Ok(_) => Ok(input.len()),
            Err(utf8_error) if utf8_error.error_len().is_some() => Err(ProtocolError::InvalidUtf8),
            Err(utf8_error) => Ok(utf8_error.valid_up_to()),
        }
    }
}

/// Retrieve the index of the last valid UTF-8 codepoint.
///
/// # Errors
///
/// Returns [`ProtocolError`] if the input is invalid UTF-8 or if the input is
/// complete but did ended on a partial codepoint.
#[cfg(not(feature = "simd"))]
#[inline]
pub fn should_fail_fast(input: &[u8], is_complete: bool) -> Result<usize, ProtocolError> {
    match std::str::from_utf8(input) {
        Ok(_) => Ok(input.len()),
        Err(utf8_error) if utf8_error.error_len().is_some() || is_complete => {
            Err(ProtocolError::InvalidUtf8)
        }
        Err(utf8_error) => Ok(utf8_error.valid_up_to()),
    }
}
