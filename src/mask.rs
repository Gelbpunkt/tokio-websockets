//! This module contains three implementations of websocket frame masking and
//! unmasking, both ways use the same algorithm and methods:
//!   - One AVX2-based implementation that masks 32 bytes per cycle
//!   - One SSE2-based implementation that masks 16 bytes per cycle
//!   - A fallback implementation without SIMD
//!
//! The SIMD implementations will only be used if the `simd` feature is active.
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
use std::{
    alloc::{alloc, dealloc, Layout},
    ptr,
};

/// AVX2 can operate on 256-bit input data.
#[cfg(all(feature = "simd", target_feature = "avx2"))]
const AVX2_ALIGNMENT: usize = 32;

/// SSE2 can operate on 128-bit input data.
#[cfg(all(
    feature = "simd",
    not(target_feature = "avx2"),
    target_feature = "sse2"
))]
const SSE2_ALIGNMENT: usize = 16;

/// (Un-)masks input bytes with the framing key using AVX2.
///
/// The input bytes may be further in the payload and therefore the offset into
/// the payload must be specified.
///
/// This will use a fallback implementation for less than 32 bytes. For
/// sufficiently large inputs, it masks in chunks of 32 bytes per instruction,
/// applying the fallback method on all remaining data.
#[cfg(all(feature = "simd", target_feature = "avx2"))]
#[inline]
pub fn frame(key: &[u8], input: &mut [u8], offset: usize) {
    unsafe {
        let payload_len = input.len();

        // We might be done already
        if payload_len < AVX2_ALIGNMENT {
            // Run fallback implementation on small data
            fallback_frame(key, input, offset);

            return;
        }

        let postamble_start = payload_len - payload_len % AVX2_ALIGNMENT;

        // Align the key so we can do an aligned load for the mask
        let layout = Layout::from_size_align_unchecked(AVX2_ALIGNMENT, AVX2_ALIGNMENT);
        let mem_ptr = alloc(layout);

        ptr::copy_nonoverlapping(key.as_ptr().add(offset), mem_ptr, 4 - offset);

        for j in (4 - offset..AVX2_ALIGNMENT).step_by(4) {
            ptr::copy_nonoverlapping(key.as_ptr(), mem_ptr.add(j), 4);
        }

        if offset > 0 {
            ptr::copy_nonoverlapping(key.as_ptr(), mem_ptr.add(AVX2_ALIGNMENT - offset), offset);
        }

        let mask = _mm256_load_si256(mem_ptr.cast());

        for index in (0..postamble_start).step_by(AVX2_ALIGNMENT) {
            let memory_addr = input.as_mut_ptr().add(index).cast();
            // Input data is not aligned on any particular boundary
            let mut v = _mm256_loadu_si256(memory_addr);
            v = _mm256_xor_si256(v, mask);
            _mm256_storeu_si256(memory_addr, v);
        }

        dealloc(mem_ptr, layout);

        if postamble_start != payload_len {
            // Run fallback implementation on postamble data
            fallback_frame(key, input.get_unchecked_mut(postamble_start..), offset);
        }
    }
}

/// (Un-)masks input bytes with the framing key using SSE2.
///
/// The input bytes may be further in the payload and therefore the offset into
/// the payload must be specified.
///
/// This will use a fallback implementation for less than 16 bytes. For
/// sufficiently large inputs, it masks in chunks of 16 bytes per instruction,
/// applying the fallback method on all remaining data.
#[cfg(all(
    feature = "simd",
    not(target_feature = "avx2"),
    target_feature = "sse2"
))]
#[inline]
pub fn frame(key: &[u8], input: &mut [u8], offset: usize) {
    unsafe {
        let payload_len = input.len();

        // We might be done already
        if payload_len < SSE2_ALIGNMENT {
            // Run fallback implementation on small data
            fallback_frame(key, input, offset);

            return;
        }

        let postamble_start = payload_len - payload_len % SSE2_ALIGNMENT;

        // Align the key so we can do an aligned load for the mask
        let layout = Layout::from_size_align_unchecked(SSE2_ALIGNMENT, SSE2_ALIGNMENT);
        let mem_ptr = alloc(layout);

        ptr::copy_nonoverlapping(key.as_ptr().add(offset), mem_ptr, 4 - offset);

        for j in (4 - offset..SSE2_ALIGNMENT).step_by(4) {
            ptr::copy_nonoverlapping(key.as_ptr(), mem_ptr.add(j), 4);
        }

        if offset > 0 {
            ptr::copy_nonoverlapping(key.as_ptr(), mem_ptr.add(SSE2_ALIGNMENT - offset), offset);
        }

        let mask = _mm_load_si128(mem_ptr.cast());

        for index in (0..postamble_start).step_by(SSE2_ALIGNMENT) {
            let memory_addr = input.as_mut_ptr().add(index).cast();
            // Input data is not aligned on any particular boundary
            let mut v = _mm_loadu_si128(memory_addr);
            v = _mm_xor_si128(v, mask);
            _mm_storeu_si128(memory_addr, v);
        }

        dealloc(mem_ptr, layout);

        if postamble_start != payload_len {
            // Run fallback implementation on postamble data
            fallback_frame(key, input.get_unchecked_mut(postamble_start..), offset);
        }
    }
}

/// (Un-)masks input bytes with the framing key.
///
/// The input bytes may be further in the payload and therefore the offset into
/// the payload must be specified.
#[cfg(not(feature = "simd"))]
#[inline]
pub fn frame(key: &[u8], input: &mut [u8], offset: usize) {
    fallback_frame(key, input, offset);
}

/// (Un-)masks input bytes with the framing key.
///
/// The input bytes may be further in the payload and therefore the offset into
/// the payload must be specified.
///
/// This is used as the internal implementation in non-SIMD builds and as a
/// fallback in SIMD builds.
pub fn fallback_frame(key: &[u8], input: &mut [u8], offset: usize) {
    for (index, byte) in input.iter_mut().enumerate() {
        *byte ^= key[(index + offset) & 3];
    }
}
