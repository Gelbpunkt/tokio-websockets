#[cfg(all(feature = "simd", target_feature = "avx2"))]
use std::arch::x86_64::{
    _mm256_load_si256, _mm256_loadu_si256, _mm256_storeu_si256, _mm256_xor_si256,
};
#[cfg(all(
    feature = "simd",
    not(target_feature = "avx2"),
    target_feature = "sse2"
))]
use std::arch::x86_64::{_mm_load_si128, _mm_loadu_si128, _mm_storeu_si128, _mm_xor_si128};
#[cfg(all(
    feature = "simd",
    any(target_feature = "avx2", target_feature = "sse2")
))]
use std::ptr;

#[cfg(all(feature = "simd", target_feature = "avx2"))]
const AVX2_ALIGNMENT: usize = 32;

#[cfg(all(
    feature = "simd",
    not(target_feature = "avx2"),
    target_feature = "sse2"
))]
const SSE2_ALIGNMENT: usize = 16;

#[cfg(all(feature = "simd", target_feature = "avx2"))]
#[inline]
pub fn frame(key: &[u8], input: &mut [u8]) {
    unsafe {
        let payload_len = input.len();

        // We might be done already
        if payload_len < AVX2_ALIGNMENT {
            // Run fallback implementation on small data
            fallback_frame(key, input);

            return;
        }

        let postamble_start = payload_len - payload_len % AVX2_ALIGNMENT;

        // Align the key
        let mut extended_mask = [0; AVX2_ALIGNMENT];

        for j in (0..AVX2_ALIGNMENT).step_by(4) {
            ptr::copy_nonoverlapping(key.as_ptr(), extended_mask.as_mut_ptr().add(j), 4);
        }

        let mask = _mm256_load_si256(extended_mask.as_ptr().cast());

        for index in (0..postamble_start).step_by(AVX2_ALIGNMENT) {
            let memory_addr = input.as_mut_ptr().add(index).cast();
            let mut v = _mm256_loadu_si256(memory_addr);
            v = _mm256_xor_si256(v, mask);
            _mm256_storeu_si256(memory_addr, v);
        }

        if postamble_start != payload_len {
            // Run fallback implementation on postamble data
            fallback_frame(key, input.get_unchecked_mut(postamble_start..))
        }
    }
}

#[cfg(all(
    feature = "simd",
    not(target_feature = "avx2"),
    target_feature = "sse2"
))]
#[inline]
pub fn frame(key: &[u8], input: &mut [u8]) {
    unsafe {
        let payload_len = input.len();

        // We might be done already
        if payload_len < SSE2_ALIGNMENT {
            // Run fallback implementation on small data
            fallback_frame(key, input);

            return;
        }

        let postamble_start = payload_len - payload_len % AVX2_ALIGNMENT;

        // Align the key
        let mut extended_mask = [0; SSE2_ALIGNMENT];

        for j in (0..SSE2_ALIGNMENT).step_by(4) {
            ptr::copy_nonoverlapping(key.as_ptr(), extended_mask.as_mut_ptr().add(j), 4);
        }

        let mask = _mm_load_si128(extended_mask.as_ptr().cast());

        for index in (0..postamble_start).step_by(SSE2_ALIGNMENT) {
            let memory_addr = input.as_mut_ptr().add(index).cast();
            let mut v = _mm_loadu_si128(memory_addr);
            v = _mm_xor_si128(v, mask);
            _mm_storeu_si128(memory_addr, v);
        }

        if postamble_start != payload_len {
            // Run fallback implementation on postamble data
            fallback_frame(key, input.get_unchecked_mut(postamble_start..));
        }
    }
}

#[cfg(not(feature = "simd"))]
#[inline]
pub fn frame(key: &[u8], input: &mut [u8]) {
    fallback_frame(key, input);
}

pub fn fallback_frame(key: &[u8], input: &mut [u8]) {
    for (index, byte) in input.iter_mut().enumerate() {
        *byte ^= key[index % 4];
    }
}
