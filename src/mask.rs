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

        // Align the key
        let aligned_to = Layout::from_size_align_unchecked(AVX2_ALIGNMENT, AVX2_ALIGNMENT);
        let mem_ptr = alloc(aligned_to);
        let mut extended_mask: [u8; AVX2_ALIGNMENT] = *(mem_ptr as *const [u8; AVX2_ALIGNMENT]);

        ptr::copy_nonoverlapping(
            key.as_ptr().add(offset),
            extended_mask.as_mut_ptr(),
            4 - offset,
        );

        for j in (4 - offset..AVX2_ALIGNMENT).step_by(4) {
            ptr::copy_nonoverlapping(key.as_ptr(), extended_mask.as_mut_ptr().add(j), 4);
        }

        ptr::copy_nonoverlapping(
            key.as_ptr(),
            extended_mask.as_mut_ptr().add(AVX2_ALIGNMENT - offset),
            offset,
        );

        let mask = _mm256_load_si256(extended_mask.as_ptr().cast());

        for index in (0..postamble_start).step_by(AVX2_ALIGNMENT) {
            let memory_addr = input.as_mut_ptr().add(index).cast();
            let mut v = _mm256_loadu_si256(memory_addr);
            v = _mm256_xor_si256(v, mask);
            _mm256_storeu_si256(memory_addr, v);
        }

        dealloc(mem_ptr, aligned_to);

        if postamble_start != payload_len {
            // Run fallback implementation on postamble data
            fallback_frame(key, input.get_unchecked_mut(postamble_start..), offset);
        }
    }
}

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

        // Align the key
        let aligned_to = Layout::from_size_align_unchecked(SSE2_ALIGNMENT, SSE2_ALIGNMENT);
        let mem_ptr = alloc(aligned_to);
        let mut extended_mask: [u8; SSE2_ALIGNMENT] = *(mem_ptr as *const [u8; SSE2_ALIGNMENT]);

        ptr::copy_nonoverlapping(
            key.as_ptr().add(offset),
            extended_mask.as_mut_ptr(),
            4 - offset,
        );

        for j in (4 - offset..SSE2_ALIGNMENT).step_by(4) {
            ptr::copy_nonoverlapping(key.as_ptr(), extended_mask.as_mut_ptr().add(j), 4);
        }

        ptr::copy_nonoverlapping(
            key.as_ptr(),
            extended_mask.as_mut_ptr().add(SSE2_ALIGNMENT - offset),
            offset,
        );

        let mask = _mm_load_si128(extended_mask.as_ptr().cast());

        for index in (0..postamble_start).step_by(SSE2_ALIGNMENT) {
            let memory_addr = input.as_mut_ptr().add(index).cast();
            let mut v = _mm_loadu_si128(memory_addr);
            v = _mm_xor_si128(v, mask);
            _mm_storeu_si128(memory_addr, v);
        }

        dealloc(mem_ptr, aligned_to);

        if postamble_start != payload_len {
            // Run fallback implementation on postamble data
            fallback_frame(key, input.get_unchecked_mut(postamble_start..), offset);
        }
    }
}

#[cfg(not(feature = "simd"))]
#[inline]
pub fn frame(key: &[u8], input: &mut [u8], offset: usize) {
    fallback_frame(key, input, offset);
}

pub fn fallback_frame(key: &[u8], input: &mut [u8], offset: usize) {
    for (index, byte) in input.iter_mut().enumerate() {
        *byte ^= key[(offset + index) & 3];
    }
}
