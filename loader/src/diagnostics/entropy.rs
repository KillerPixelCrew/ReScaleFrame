// SPDX-License-Identifier: MIT
use std::{ffi::c_void, slice};

pub(super) fn entropy(bytes: &[u8]) -> f64 {
    let mut counts = [0usize; 256];
    for &byte in bytes {
        counts[usize::from(byte)] += 1;
    }
    counts
        .iter()
        .filter(|&&count| count != 0)
        .fold(0.0, |entropy, &count| {
            let probability = count as f64 / bytes.len() as f64;
            entropy - probability * probability.log2()
        })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_shannon_entropy(data: *const c_void, size: usize) -> f64 {
    if data.is_null() || size == 0 || size > isize::MAX as usize {
        return 0.0;
    }
    entropy(unsafe { slice::from_raw_parts(data.cast(), size) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_sampled_entropy(
    data: *const c_void,
    size: usize,
    window: usize,
    samples: usize,
) -> f64 {
    if data.is_null() || size == 0 || size > isize::MAX as usize || window == 0 || samples == 0 {
        return 0.0;
    }
    let bytes = unsafe { slice::from_raw_parts(data.cast(), size) };
    sampled(size, window, samples, |offset, length| {
        entropy(&bytes[offset..offset + length])
    })
}

pub(super) fn sampled(
    size: usize,
    window: usize,
    samples: usize,
    mut measure: impl FnMut(usize, usize) -> f64,
) -> f64 {
    if size <= window {
        return measure(0, size);
    }
    let span = (size - window) as u128;
    let mut total = 0.0;
    for index in 0..samples {
        // Include both endpoints without overflowing when multiplying the span by the index.
        let offset = if samples == 1 {
            0
        } else {
            (span * index as u128 / (samples - 1) as u128) as usize
        };
        total += measure(offset, window);
    }
    total / samples as f64
}
