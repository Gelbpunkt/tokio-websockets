//! This module contains three implementations of websocket frame masking and
//! unmasking, both ways use the same algorithm and methods:
//!   - One AVX512-based implementation that masks 64 bytes per cycle (requires
//!     nightly rust)
//!   - One AVX2-based implementation that masks 32 bytes per cycle
//!   - One SSE2-based implementation that masks 16 bytes per cycle
//!   - One NEON-based implementation that masks 16 bytes per cycle
//!   - A fallback implementation without SIMD
//!
//! The SIMD implementations will only be used if the `simd` feature is active.
#[cfg(all(feature = "simd", target_arch = "aarch64", target_feature = "neon"))]
use std::arch::aarch64::{uint8x16_t, veorq_u8, vld1q_u8};
#[cfg(all(feature = "simd", target_arch = "arm", target_feature = "neon"))]
use std::arch::arm::{uint8x16_t, veorq_u8, vld1q_u8};
#[cfg(all(
    feature = "simd",
    not(all(feature = "nightly", target_feature = "avx512f")),
    not(target_feature = "avx2"),
    target_feature = "sse2"
))]
use std::arch::x86_64::{__m128i, _mm_load_si128, _mm_xor_si128};
#[cfg(all(
    feature = "simd",
    not(all(feature = "nightly", target_feature = "avx512f")),
    target_feature = "avx2"
))]
use std::arch::x86_64::{__m256i, _mm256_load_si256, _mm256_xor_si256};
#[cfg(all(feature = "simd", feature = "nightly", target_feature = "avx512f"))]
use std::arch::x86_64::{__m512i, _mm512_load_si512, _mm512_xor_si512};
#[cfg(all(
    feature = "simd",
    any(
        target_feature = "avx2",
        target_feature = "sse2",
        all(feature = "nightly", target_feature = "avx512f"),
        target_feature = "neon"
    )
))]
use std::{
    alloc::{alloc, dealloc, Layout},
    ptr,
};

/// AVX512 can operate on 512-bit input data.
#[cfg(all(feature = "simd", feature = "nightly", target_feature = "avx512f"))]
const AVX512_ALIGNMENT: usize = 64;

/// AVX2 can operate on 256-bit input data.
#[cfg(all(
    feature = "simd",
    not(all(feature = "nightly", target_feature = "avx512f")),
    target_feature = "avx2"
))]
const AVX2_ALIGNMENT: usize = 32;

/// SSE2 can operate on 128-bit input data.
#[cfg(all(
    feature = "simd",
    not(all(feature = "nightly", target_feature = "avx512f")),
    not(target_feature = "avx2"),
    target_feature = "sse2"
))]
const SSE2_ALIGNMENT: usize = 16;

/// NEON can operate on 128-bit input data.
#[cfg(all(feature = "simd", target_feature = "neon"))]
const NEON_ALIGNMENT: usize = 16;

/// (Un-)masks input bytes with the framing key using AVX512.
///
/// The input bytes may be further in the payload and therefore the offset into
/// the payload must be specified.
///
/// This will use a fallback implementation for less than 64 bytes. For
/// sufficiently large inputs, it masks in chunks of 64 bytes per instruction,
/// applying the fallback method on all remaining data.
#[cfg(all(feature = "simd", feature = "nightly", target_feature = "avx512f"))]
#[inline]
pub fn frame(key: &[u8], input: &mut [u8], mut offset: usize) {
    unsafe {
        let (prefix, aligned_data, suffix) = input.align_to_mut::<__m512i>();

        // Run fallback implementation on unaligned prefix data
        fallback_frame(key, prefix, offset);
        offset = (offset + prefix.len()) & 3;

        // Align the key so we can do an aligned load for the mask
        let layout = Layout::from_size_align_unchecked(AVX512_ALIGNMENT, AVX512_ALIGNMENT);
        let mem_ptr = alloc(layout);

        ptr::copy_nonoverlapping(key.as_ptr().add(offset), mem_ptr, 4 - offset);

        for j in (4 - offset..AVX512_ALIGNMENT).step_by(4) {
            ptr::copy_nonoverlapping(key.as_ptr(), mem_ptr.add(j), 4);
        }

        if offset > 0 {
            ptr::copy_nonoverlapping(key.as_ptr(), mem_ptr.add(AVX512_ALIGNMENT - offset), offset);
        }

        let mask = _mm512_load_si512(mem_ptr.cast());

        for block in &mut *aligned_data {
            *block = _mm512_xor_si512(*block, mask);
        }

        dealloc(mem_ptr, layout);

        // Run fallback implementation on unaligned suffix data
        offset = (offset + aligned_data.len() * AVX512_ALIGNMENT) & 3;
        fallback_frame(key, suffix, offset);
    }
}

/// (Un-)masks input bytes with the framing key using AVX2.
///
/// The input bytes may be further in the payload and therefore the offset into
/// the payload must be specified.
///
/// This will use a fallback implementation for less than 32 bytes. For
/// sufficiently large inputs, it masks in chunks of 32 bytes per instruction,
/// applying the fallback method on all remaining data.
#[cfg(all(
    feature = "simd",
    not(all(feature = "nightly", target_feature = "avx512f")),
    target_feature = "avx2"
))]
#[inline]
pub fn frame(key: &[u8], input: &mut [u8], mut offset: usize) {
    unsafe {
        let (prefix, aligned_data, suffix) = input.align_to_mut::<__m256i>();

        // Run fallback implementation on unaligned prefix data
        fallback_frame(key, prefix, offset);
        offset = (offset + prefix.len()) & 3;

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

        for block in &mut *aligned_data {
            *block = _mm256_xor_si256(*block, mask);
        }

        dealloc(mem_ptr, layout);

        // Run fallback implementation on unaligned suffix data
        offset = (offset + aligned_data.len() * AVX2_ALIGNMENT) & 3;
        fallback_frame(key, suffix, offset);
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
    not(all(feature = "nightly", target_feature = "avx512f")),
    not(target_feature = "avx2"),
    target_feature = "sse2"
))]
#[inline]
pub fn frame(key: &[u8], input: &mut [u8], mut offset: usize) {
    unsafe {
        let (prefix, aligned_data, suffix) = input.align_to_mut::<__m128i>();

        // Run fallback implementation on unaligned prefix data
        fallback_frame(key, prefix, offset);
        offset = (offset + prefix.len()) & 3;

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

        for block in &mut *aligned_data {
            *block = _mm_xor_si128(*block, mask);
        }

        dealloc(mem_ptr, layout);

        // Run fallback implementation on unaligned suffix data
        offset = (offset + aligned_data.len() * SSE2_ALIGNMENT) & 3;
        fallback_frame(key, suffix, offset);
    }
}

#[cfg(all(feature = "simd", target_feature = "neon"))]
#[inline]
pub fn frame(key: &[u8], input: &mut [u8], mut offset: usize) {
    unsafe {
        let (prefix, aligned_data, suffix) = input.align_to_mut::<uint8x16_t>();

        // Run fallback implementation on unaligned prefix data
        fallback_frame(key, prefix, offset);
        offset = (offset + prefix.len()) & 3;

        // vld1q_u8 has no alignment requirements whatsoever, ARM accepts unaligned
        // pointers. However, it seems that on 32-bit ARM, alignment is an optional
        // parameter in the instruction. It does not really hurt to align it to allow
        // the compiler to generate the optimized 32-bit instructions just in case it
        // matters.
        let layout = Layout::from_size_align_unchecked(NEON_ALIGNMENT, NEON_ALIGNMENT);
        let mem_ptr = alloc(layout);

        ptr::copy_nonoverlapping(key.as_ptr().add(offset), mem_ptr, 4 - offset);

        for j in (4 - offset..NEON_ALIGNMENT).step_by(4) {
            ptr::copy_nonoverlapping(key.as_ptr(), mem_ptr.add(j), 4);
        }

        if offset > 0 {
            ptr::copy_nonoverlapping(key.as_ptr(), mem_ptr.add(NEON_ALIGNMENT - offset), offset);
        }

        let mask = vld1q_u8(mem_ptr);

        for block in &mut *aligned_data {
            *block = veorq_u8(*block, mask);
        }

        dealloc(mem_ptr, layout);

        // Run fallback implementation on unaligned suffix data
        offset = (offset + aligned_data.len() * NEON_ALIGNMENT) & 3;
        fallback_frame(key, suffix, offset);
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

#[test]
fn test_mask() {
    use crate::rand::get_mask;

    let data: Vec<u8> = std::iter::repeat_with(|| fastrand::u8(..))
        .take(1024)
        .collect();
    // Mess around with the data to ensure we have unaligned input
    let mut data = data[2..998].to_vec();
    let mut data_clone = data.clone();
    let key = get_mask();
    frame(&key, &mut data, 0);
    fallback_frame(&key, &mut data_clone, 0);

    assert_eq!(&data, &data_clone);
}
