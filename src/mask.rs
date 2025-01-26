//! This module contains six implementations of WebSocket frame masking and
//! unmasking, all of them using the same algorithm and methods:
//!   - One AVX512-based implementation that masks 64 bytes per cycle (requires
//!     nightly rust)
//!   - One AVX2-based implementation that masks 32 bytes per cycle
//!   - One SSE2-based implementation that masks 16 bytes per cycle
//!   - One NEON-based implementation that masks 16 bytes per cycle (requires
//!     nightly rust on 32-bit ARM)
//!   - One AltiVec-based implementation that masks 16 bytes per cycle (requires
//!     nightly rust)
//!   - A fallback implementation that masks 8 bytes per cycle
//!
//! The SIMD implementations will be used if CPU support for them is detected at
//! runtime or enabled at compile time via compiler flags.

/// (Un-)masks input bytes with the framing key using AVX512.
///
/// The input bytes may be further in the payload and therefore the offset
/// into the payload must be specified.
///
/// This will use a fallback implementation for less than 64 bytes. For
/// sufficiently large inputs, it masks in chunks of 64 bytes per
/// instruction, applying the fallback method on all remaining data.
#[cfg(all(feature = "nightly", any(target_arch = "x86", target_arch = "x86_64")))]
#[target_feature(enable = "avx512f")]
unsafe fn frame_avx512(key: [u8; 4], input: &mut [u8], mut offset: usize) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{__m512i, _mm512_set1_epi32, _mm512_xor_si512};
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{__m512i, _mm512_set1_epi32, _mm512_xor_si512};

    unsafe {
        let (prefix, aligned_data, suffix) = input.align_to_mut::<__m512i>();

        // Run fallback implementation on unaligned prefix data
        fallback_frame(key, prefix, offset);
        offset = (offset + prefix.len()) % key.len();

        if !aligned_data.is_empty() {
            #[allow(clippy::cast_possible_truncation)] // offset is 0..4
            let mask = _mm512_set1_epi32(
                i32::from_be_bytes(key)
                    .rotate_left(offset as u32 * u8::BITS)
                    .to_be(),
            );

            for block in aligned_data {
                *block = _mm512_xor_si512(*block, mask);
            }
        }

        // Run fallback implementation on unaligned suffix data
        fallback_frame(key, suffix, offset);
    }
}

/// (Un-)masks input bytes with the framing key using AVX2.
///
/// The input bytes may be further in the payload and therefore the offset
/// into the payload must be specified.
///
/// This will use a fallback implementation for less than 32 bytes. For
/// sufficiently large inputs, it masks in chunks of 32 bytes per
/// instruction, applying the fallback method on all remaining data.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn frame_avx2(key: [u8; 4], input: &mut [u8], mut offset: usize) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{__m256i, _mm256_set1_epi32, _mm256_xor_si256};
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{__m256i, _mm256_set1_epi32, _mm256_xor_si256};

    unsafe {
        let (prefix, aligned_data, suffix) = input.align_to_mut::<__m256i>();

        // Run fallback implementation on unaligned prefix data
        fallback_frame(key, prefix, offset);
        offset = (offset + prefix.len()) % key.len();

        if !aligned_data.is_empty() {
            #[allow(clippy::cast_possible_truncation)] // offset is 0..4
            let mask = _mm256_set1_epi32(
                i32::from_be_bytes(key)
                    .rotate_left(offset as u32 * u8::BITS)
                    .to_be(),
            );

            for block in aligned_data {
                *block = _mm256_xor_si256(*block, mask);
            }
        }

        // Run fallback implementation on unaligned suffix data
        fallback_frame(key, suffix, offset);
    }
}

/// (Un-)masks input bytes with the framing key using SSE2.
///
/// The input bytes may be further in the payload and therefore the offset
/// into the payload must be specified.
///
/// This will use a fallback implementation for less than 16 bytes. For
/// sufficiently large inputs, it masks in chunks of 16 bytes per
/// instruction, applying the fallback method on all remaining data.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
unsafe fn frame_sse2(key: [u8; 4], input: &mut [u8], mut offset: usize) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{__m128i, _mm_set1_epi32, _mm_xor_si128};
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{__m128i, _mm_set1_epi32, _mm_xor_si128};

    unsafe {
        let (prefix, aligned_data, suffix) = input.align_to_mut::<__m128i>();

        // Run fallback implementation on unaligned prefix data
        fallback_frame(key, prefix, offset);
        offset = (offset + prefix.len()) % key.len();

        if !aligned_data.is_empty() {
            #[allow(clippy::cast_possible_truncation)] // offset is 0..4
            let mask = _mm_set1_epi32(
                i32::from_be_bytes(key)
                    .rotate_left(offset as u32 * u8::BITS)
                    .to_be(),
            );

            for block in aligned_data {
                *block = _mm_xor_si128(*block, mask);
            }
        }

        // Run fallback implementation on unaligned suffix data
        fallback_frame(key, suffix, offset);
    }
}

/// (Un-)masks input bytes with the framing key using NEON.
///
/// The input bytes may be further in the payload and therefore the offset
/// into the payload must be specified.
///
/// This will use a fallback implementation for less than 16 bytes. For
/// sufficiently large inputs, it masks in chunks of 16 bytes per
/// instruction, applying the fallback method on all remaining data.
#[cfg(any(all(feature = "nightly", target_arch = "arm"), target_arch = "aarch64"))]
#[target_feature(enable = "neon")]
unsafe fn frame_neon(key: [u8; 4], input: &mut [u8], mut offset: usize) {
    #[cfg(target_arch = "aarch64")]
    use std::arch::aarch64::{uint8x16_t, veorq_u8, vld1q_dup_s32, vreinterpretq_u8_s32};
    #[cfg(target_arch = "arm")]
    use std::arch::arm::{uint8x16_t, veorq_u8, vld1q_dup_s32, vreinterpretq_u8_s32};

    unsafe {
        let (prefix, aligned_data, suffix) = input.align_to_mut::<uint8x16_t>();

        // Run fallback implementation on unaligned prefix data
        fallback_frame(key, prefix, offset);
        offset = (offset + prefix.len()) % key.len();

        if !aligned_data.is_empty() {
            #[allow(clippy::cast_possible_truncation)] // offset is 0..4
            let mask = vreinterpretq_u8_s32(vld1q_dup_s32(
                &i32::from_be_bytes(key)
                    .rotate_left(offset as u32 * u8::BITS)
                    .to_be() as *const i32,
            ));

            for block in aligned_data {
                *block = veorq_u8(*block, mask);
            }
        }

        // Run fallback implementation on unaligned suffix data
        fallback_frame(key, suffix, offset);
    }
}

/// (Un-)masks input bytes with the framing key using AltiVec.
///
/// The input bytes may be further in the payload and therefore the offset
/// into the payload must be specified.
///
/// This will use a fallback implementation for less than 16 bytes. For
/// sufficiently large inputs, it masks in chunks of 16 bytes per
/// instruction, applying the fallback method on all remaining data.
#[cfg(all(
    feature = "nightly",
    any(target_arch = "powerpc", target_arch = "powerpc64")
))]
#[target_feature(enable = "altivec")]
unsafe fn frame_altivec(key: [u8; 4], input: &mut [u8], mut offset: usize) {
    #[cfg(target_arch = "powerpc")]
    use std::arch::powerpc::{vec_splats, vec_xor, vector_unsigned_char};
    #[cfg(target_arch = "powerpc64")]
    use std::arch::powerpc64::{vec_splats, vec_xor, vector_unsigned_char};
    use std::mem::transmute;

    unsafe {
        let (prefix, aligned_data, suffix) = input.align_to_mut::<vector_unsigned_char>();

        // Run fallback implementation on unaligned prefix data
        fallback_frame(key, prefix, offset);
        offset = (offset + prefix.len()) % key.len();

        if !aligned_data.is_empty() {
            // SAFETY: 4x i32 to 16x u8 is safe
            #[allow(clippy::cast_possible_truncation)] // offset is 0..4
            let mask: vector_unsigned_char = transmute(vec_splats(
                i32::from_be_bytes(key)
                    .rotate_left(offset as u32 * u8::BITS)
                    .to_be(),
            ));

            for block in aligned_data {
                *block = vec_xor(*block, mask);
            }
        }

        // Run fallback implementation on unaligned suffix data
        fallback_frame(key, suffix, offset);
    }
}

/// (Un-)masks input bytes with the framing key, one byte at once.
///
/// See [`fallback_frame`] for more details.
fn one_byte_at_once(key: [u8; 4], input: &mut [u8], offset: usize) {
    // This lets the compiler unroll the loop partially compared to adding the
    // offset in the loop. The assembly is more readable this way and easier to
    // reason about, performance wise it's roundabout the same because the compiler
    // otherwise generates a garbled mess of special cases.
    #[allow(clippy::cast_possible_truncation)] // offset is 0..4
    let key = i32::from_be_bytes(key)
        .rotate_left(offset as u32 * u8::BITS)
        .to_be_bytes();

    for (index, byte) in input.iter_mut().enumerate() {
        *byte ^= key[index % key.len()];
    }
}

/// (Un-)masks input bytes with the framing key.
///
/// The input bytes may be further in the payload and therefore the offset into
/// the payload must be specified.
///
/// This is used as the internal implementation in non-SIMD builds and as a
/// fallback in SIMD builds.
fn fallback_frame(key: [u8; 4], input: &mut [u8], mut offset: usize) {
    let (prefix, aligned_data, suffix) = unsafe { input.align_to_mut::<u64>() };

    // Run fallback implementation on unaligned prefix data
    one_byte_at_once(key, prefix, offset);
    offset = (offset + prefix.len()) % key.len();

    if !aligned_data.is_empty() {
        #[allow(clippy::cast_possible_truncation)] // offset is 0..4
        let masking_key = u64::from(
            u32::from_be_bytes(key)
                .rotate_left(offset as u32 * u8::BITS)
                .to_be(),
        );
        let mask = (masking_key << u32::BITS) | masking_key;

        for block in aligned_data {
            *block ^= mask;
        }
    }

    // Run fallback implementation on unaligned suffix data
    one_byte_at_once(key, suffix, offset);
}

/// (Un-)masks input bytes with the framing key.
///
/// The input bytes may be further in the payload and therefore the offset
/// into the payload must be specified.
#[inline]
pub fn frame(key: [u8; 4], input: &mut [u8], offset: usize) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        use std::arch::is_x86_feature_detected;

        #[cfg(feature = "nightly")]
        if is_x86_feature_detected!("avx512f") {
            return unsafe { frame_avx512(key, input, offset) };
        }

        if is_x86_feature_detected!("avx2") {
            return unsafe { frame_avx2(key, input, offset) };
        } else if is_x86_feature_detected!("sse2") {
            return unsafe { frame_sse2(key, input, offset) };
        }
    }

    #[cfg(all(feature = "nightly", target_arch = "arm"))]
    {
        use std::arch::is_arm_feature_detected;

        if is_arm_feature_detected!("neon") {
            return unsafe { frame_neon(key, input, offset) };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        use std::arch::is_aarch64_feature_detected;

        if is_aarch64_feature_detected!("neon") {
            return unsafe { frame_neon(key, input, offset) };
        }
    }

    #[cfg(all(feature = "nightly", target_arch = "powerpc"))]
    {
        use std::arch::is_powerpc_feature_detected;

        if is_powerpc_feature_detected!("altivec") {
            return unsafe { frame_altivec(key, input, offset) };
        }
    }

    #[cfg(all(feature = "nightly", target_arch = "powerpc64"))]
    {
        use std::arch::is_powerpc64_feature_detected;

        if is_powerpc64_feature_detected!("altivec") {
            return unsafe { frame_altivec(key, input, offset) };
        }
    }

    fallback_frame(key, input, offset);
}

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
    let mut mask = [0; 4];
    get_mask(&mut mask);
    frame(mask, &mut data, 0);
    one_byte_at_once(mask, &mut data_clone, 0);

    assert_eq!(&data, &data_clone);
}
