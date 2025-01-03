//! This module contains three implementations of WebSocket frame masking and
//! unmasking, both ways use the same algorithm and methods:
//!   - One AVX512-based implementation that masks 64 bytes per cycle (requires
//!     nightly rust)
//!   - One AVX2-based implementation that masks 32 bytes per cycle
//!   - One SSE2-based implementation that masks 16 bytes per cycle
//!   - One NEON-based implementation that masks 16 bytes per cycle (requires
//!     nightly rust on 32-bit ARM)
//!   - One AltiVec-based implementation that masks 16 bytes per cycle (requires
//!     nightly rust)
//!   - A fallback implementation without SIMD
//!
//! The SIMD implementations will only be used if the `simd` feature is active.

/// Websocket frame masking implementation using AVX512.
#[cfg(all(feature = "simd", feature = "nightly", target_feature = "avx512f"))]
mod imp {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{__m512i, _mm512_set1_epi32, _mm512_xor_si512};
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{__m512i, _mm512_set1_epi32, _mm512_xor_si512};

    /// (Un-)masks input bytes with the framing key using AVX512.
    ///
    /// The input bytes may be further in the payload and therefore the offset
    /// into the payload must be specified.
    ///
    /// This will use a fallback implementation for less than 64 bytes. For
    /// sufficiently large inputs, it masks in chunks of 64 bytes per
    /// instruction, applying the fallback method on all remaining data.
    pub fn frame(mut key: [u8; 4], input: &mut [u8], mut offset: usize) {
        unsafe {
            let (prefix, aligned_data, suffix) = input.align_to_mut::<__m512i>();

            // Run fallback implementation on unaligned prefix data
            super::fallback_frame(key, prefix, offset);
            offset = (offset + prefix.len()) % key.len();

            if !aligned_data.is_empty() {
                key.rotate_left(offset);
                offset = 0;
                let mask = _mm512_set1_epi32(i32::from_le_bytes(key));

                for block in &mut *aligned_data {
                    *block = _mm512_xor_si512(*block, mask);
                }
            }

            // Run fallback implementation on unaligned suffix data
            super::fallback_frame(key, suffix, offset);
        }
    }
}

/// Websocket frame masking implementation using AVX2.
#[cfg(all(
    feature = "simd",
    not(all(feature = "nightly", target_feature = "avx512f")),
    target_feature = "avx2"
))]
mod imp {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{__m256i, _mm256_set1_epi32, _mm256_xor_si256};
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{__m256i, _mm256_set1_epi32, _mm256_xor_si256};

    /// (Un-)masks input bytes with the framing key using AVX2.
    ///
    /// The input bytes may be further in the payload and therefore the offset
    /// into the payload must be specified.
    ///
    /// This will use a fallback implementation for less than 32 bytes. For
    /// sufficiently large inputs, it masks in chunks of 32 bytes per
    /// instruction, applying the fallback method on all remaining data.
    pub fn frame(mut key: [u8; 4], input: &mut [u8], mut offset: usize) {
        unsafe {
            let (prefix, aligned_data, suffix) = input.align_to_mut::<__m256i>();

            // Run fallback implementation on unaligned prefix data
            super::fallback_frame(key, prefix, offset);
            offset = (offset + prefix.len()) % key.len();

            if !aligned_data.is_empty() {
                key.rotate_left(offset);
                offset = 0;
                let mask = _mm256_set1_epi32(i32::from_le_bytes(key));

                for block in &mut *aligned_data {
                    *block = _mm256_xor_si256(*block, mask);
                }
            }

            // Run fallback implementation on unaligned suffix data
            super::fallback_frame(key, suffix, offset);
        }
    }
}

/// Websocket frame masking implementation using SSE2.
#[cfg(all(
    feature = "simd",
    not(all(feature = "nightly", target_feature = "avx512f")),
    not(target_feature = "avx2"),
    target_feature = "sse2"
))]
mod imp {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{__m128i, _mm_set1_epi32, _mm_xor_si128};
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{__m128i, _mm_set1_epi32, _mm_xor_si128};

    /// (Un-)masks input bytes with the framing key using SSE2.
    ///
    /// The input bytes may be further in the payload and therefore the offset
    /// into the payload must be specified.
    ///
    /// This will use a fallback implementation for less than 16 bytes. For
    /// sufficiently large inputs, it masks in chunks of 16 bytes per
    /// instruction, applying the fallback method on all remaining data.
    pub fn frame(mut key: [u8; 4], input: &mut [u8], mut offset: usize) {
        unsafe {
            let (prefix, aligned_data, suffix) = input.align_to_mut::<__m128i>();

            // Run fallback implementation on unaligned prefix data
            super::fallback_frame(key, prefix, offset);
            offset = (offset + prefix.len()) % key.len();

            if !aligned_data.is_empty() {
                key.rotate_left(offset);
                offset = 0;
                let mask = _mm_set1_epi32(i32::from_le_bytes(key));

                for block in &mut *aligned_data {
                    *block = _mm_xor_si128(*block, mask);
                }
            }

            // Run fallback implementation on unaligned suffix data
            super::fallback_frame(key, suffix, offset);
        }
    }
}

/// Websocket frame masking implementation using NEON.
#[cfg(all(
    feature = "simd",
    target_feature = "neon",
    any(target_arch = "aarch64", all(target_arch = "arm", feature = "nightly"))
))]
mod imp {
    #[cfg(target_arch = "aarch64")]
    use std::arch::aarch64::{uint8x16_t, veorq_u8, vld1q_dup_s32, vreinterpretq_u8_s32};
    #[cfg(target_arch = "arm")]
    use std::arch::arm::{uint8x16_t, veorq_u8, vld1q_dup_s32, vreinterpretq_u8_s32};

    /// (Un-)masks input bytes with the framing key using NEON.
    ///
    /// The input bytes may be further in the payload and therefore the offset
    /// into the payload must be specified.
    ///
    /// This will use a fallback implementation for less than 16 bytes. For
    /// sufficiently large inputs, it masks in chunks of 16 bytes per
    /// instruction, applying the fallback method on all remaining data.
    pub fn frame(mut key: [u8; 4], input: &mut [u8], mut offset: usize) {
        unsafe {
            let (prefix, aligned_data, suffix) = input.align_to_mut::<uint8x16_t>();

            // Run fallback implementation on unaligned prefix data
            super::fallback_frame(key, prefix, offset);
            offset = (offset + prefix.len()) % key.len();

            if !aligned_data.is_empty() {
                key.rotate_left(offset);
                offset = 0;
                let mask =
                    vreinterpretq_u8_s32(vld1q_dup_s32(&i32::from_ne_bytes(key) as *const i32));

                for block in &mut *aligned_data {
                    *block = veorq_u8(*block, mask);
                }
            }

            // Run fallback implementation on unaligned suffix data
            super::fallback_frame(key, suffix, offset);
        }
    }
}

/// Websocket frame masking implementation using AltiVec.
#[cfg(all(feature = "simd", feature = "nightly", target_feature = "altivec"))]
mod imp {
    #[cfg(target_arch = "powerpc")]
    use std::arch::powerpc::{vec_splats, vec_xor, vector_unsigned_char};
    #[cfg(target_arch = "powerpc64")]
    use std::arch::powerpc64::{vec_splats, vec_xor, vector_unsigned_char};
    use std::mem::transmute;

    /// (Un-)masks input bytes with the framing key using AltiVec.
    ///
    /// The input bytes may be further in the payload and therefore the offset
    /// into the payload must be specified.
    ///
    /// This will use a fallback implementation for less than 16 bytes. For
    /// sufficiently large inputs, it masks in chunks of 16 bytes per
    /// instruction, applying the fallback method on all remaining data.
    pub fn frame(mut key: [u8; 4], input: &mut [u8], mut offset: usize) {
        unsafe {
            let (prefix, aligned_data, suffix) = input.align_to_mut::<vector_unsigned_char>();

            // Run fallback implementation on unaligned prefix data
            super::fallback_frame(key, prefix, offset);
            offset = (offset + prefix.len()) % key.len();

            if !aligned_data.is_empty() {
                key.rotate_left(offset);
                offset = 0;
                // SAFETY: 4x i32 to 16x u8 is safe
                let mask: vector_unsigned_char = transmute(vec_splats(i32::from_ne_bytes(key)));

                for block in &mut *aligned_data {
                    *block = vec_xor(*block, mask);
                }
            }

            // Run fallback implementation on unaligned suffix data
            super::fallback_frame(key, suffix, offset);
        }
    }
}

/// Websocket frame masking fallback implementation.
#[cfg(any(
    not(feature = "simd"),
    all(
        feature = "simd",
        any(
            all(target_arch = "aarch64", not(target_feature = "neon")),
            all(
                target_arch = "arm",
                not(all(target_feature = "neon", feature = "nightly"))
            )
        )
    ),
    all(
        feature = "simd",
        any(target_arch = "powerpc64", target_arch = "powerpc"),
        any(not(target_feature = "altivec"), not(feature = "nightly"))
    ),
    not(any(
        target_arch = "x86_64",
        target_arch = "x86",
        target_arch = "aarch64",
        target_arch = "arm",
        target_arch = "powerpc64",
        target_arch = "powerpc"
    ))
))]
mod imp {
    /// (Un-)masks input bytes with the framing key.
    ///
    /// The input bytes may be further in the payload and therefore the offset
    /// into the payload must be specified.
    #[inline]
    pub fn frame(key: [u8; 4], input: &mut [u8], offset: usize) {
        super::fallback_frame(key, input, offset);
    }
}

/// (Un-)masks input bytes with the framing key.
///
/// The input bytes may be further in the payload and therefore the offset into
/// the payload must be specified.
///
/// This is used as the internal implementation in non-SIMD builds and as a
/// fallback in SIMD builds.
pub fn fallback_frame(key: [u8; 4], input: &mut [u8], offset: usize) {
    for (index, byte) in input.iter_mut().enumerate() {
        *byte ^= key[(index + offset) % key.len()];
    }
}

pub use imp::frame;

#[cfg(all(test, feature = "client", feature = "fastrand"))]
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
    frame(key, &mut data, 0);
    fallback_frame(key, &mut data_clone, 0);

    assert_eq!(&data, &data_clone);
}
