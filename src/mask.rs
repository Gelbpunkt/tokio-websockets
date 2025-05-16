//! This module contains the websocket frame (un-)masking implementation.

/// (Un-)masks input bytes with the framing key, one byte at once.
#[allow(clippy::cast_possible_truncation)] // offset % 4 is within u32 bounds
fn one_byte_at_once(key: &mut [u8; 4], input: &mut [u8], is_complete: bool) {
    for (index, byte) in input.iter_mut().enumerate() {
        *byte ^= key[index % key.len()];
    }

    // Rotate the mask by the required number of bytes
    if !is_complete {
        *key = u32::from_be_bytes(*key)
            .rotate_left((input.len() % key.len()) as u32 * u8::BITS)
            .to_be_bytes();
    }
}

/// (Un-)masks input bytes with the framing key.
pub fn frame(key: &mut [u8; 4], input: &mut [u8], is_complete: bool) {
    // The one byte at once method is way faster for small inputs
    if input.len() < 4 {
        return one_byte_at_once(key, input, is_complete);
    }

    let (prefix, aligned_data, suffix) = unsafe { input.align_to_mut::<u64>() };

    // Run fallback implementation on unaligned prefix data
    one_byte_at_once(key, prefix, false);

    if !aligned_data.is_empty() {
        let masking_key = u64::from(u32::from_ne_bytes(*key));
        let mask = (masking_key << u32::BITS) | masking_key;

        for block in aligned_data {
            *block ^= mask;
        }
    }

    // Run fallback implementation on unaligned suffix data
    one_byte_at_once(key, suffix, is_complete);
}

#[cfg(all(test, feature = "client", feature = "fastrand"))]
#[test]
fn test_mask() {
    use crate::rand::get_mask;

    let mut random_data: Vec<u8> = std::iter::repeat_with(|| fastrand::u8(..))
        .take(1024)
        .collect();
    // Mess around with the data to ensure we have unaligned input
    let data = &mut random_data[2..998];
    let mut data_clone = data.to_vec();
    let mut mask = [0; 4];
    get_mask(&mut mask);
    let mut mask2 = mask;
    frame(&mut mask, data, true);
    one_byte_at_once(&mut mask2, &mut data_clone, true);

    assert_eq!(&data, &data_clone);
}
