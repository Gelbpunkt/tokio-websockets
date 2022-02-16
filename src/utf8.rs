use crate::proto::ProtocolError;

#[cfg(feature = "simd")]
#[inline]
pub fn parse_str(input: &[u8]) -> Result<&str, ProtocolError> {
    match simdutf8::basic::from_utf8(input) {
        Ok(string) => Ok(string),
        Err(_) => Err(ProtocolError::InvalidUtf8),
    }
}

#[cfg(not(feature = "simd"))]
#[inline]
pub fn parse_str(input: &[u8]) -> Result<&str, ProtocolError> {
    Ok(std::str::from_utf8(input)?)
}

#[cfg(feature = "simd")]
#[inline]
pub fn should_fail_fast(input: &[u8], is_complete: bool) -> (bool, usize) {
    match simdutf8::compat::from_utf8(input) {
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
