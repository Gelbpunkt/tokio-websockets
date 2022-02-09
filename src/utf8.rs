#[cfg(feature = "simd")]
use simdutf8::basic::imp::Utf8Validator;

use crate::proto::ProtocolError;

#[cfg(feature = "simd")]
#[inline]
pub fn parse(input: Vec<u8>) -> Result<String, ProtocolError> {
    unsafe {
        #[cfg(target_feature = "avx2")]
        let mut validator = simdutf8::basic::imp::x86::avx2::Utf8ValidatorImp::new();
        #[cfg(all(target_feature = "sse4.2", not(target_feature = "avx2")))]
        let mut validator = simdutf8::basic::imp::x86::sse42::Utf8ValidatorImp::new();

        validator.update(&input);

        if validator.finalize().is_ok() {
            Ok(String::from_utf8_unchecked(input))
        } else {
            Err(ProtocolError::InvalidUtf8)
        }
    }
}

#[cfg(not(feature = "simd"))]
#[inline]
pub fn parse(input: Vec<u8>) -> Result<String, ProtocolError> {
    Ok(String::from_utf8(input)?)
}

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
pub fn should_fail_fast(input: &[u8], is_complete: bool) -> bool {
    match simdutf8::compat::from_utf8(input) {
        Ok(_) => false,
        Err(utf8_error) => {
            if is_complete {
                true
            } else {
                utf8_error.error_len().is_some()
            }
        }
    }
}

#[cfg(not(feature = "simd"))]
#[inline]
pub fn should_fail_fast(input: &[u8], is_complete: bool) -> bool {
    match std::str::from_utf8(input) {
        Ok(_) => false,
        Err(utf8_error) => {
            if is_complete {
                true
            } else {
                utf8_error.error_len().is_some()
            }
        }
    }
}
