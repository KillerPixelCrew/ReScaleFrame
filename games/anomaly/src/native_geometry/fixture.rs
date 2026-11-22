// SPDX-License-Identifier: MIT
//! Seeds the synthetic executable for the native MinHook fixture.

use crate::engine_guards::decode_hex;
use crate::native_geometry_abi::GuardProfile;
use std::ffi::CStr;

fn write_at(image: &mut [u8], rva: u32, bytes: &[u8]) -> Option<()> {
    let start = rva as usize;
    let end = start.checked_add(bytes.len())?;
    image.get_mut(start..end)?.copy_from_slice(bytes);
    Some(())
}

fn seed(image: &mut [u8], profile: &GuardProfile) -> Option<()> {
    for site in profile.sites().iter().chain(profile.layout_sites()) {
        write_at(image, site.rva, &site.expected)?;
    }
    for guard in profile.unique() {
        // Each bank lends immutable, NUL-terminated guard strings.
        let hex = unsafe { CStr::from_ptr(guard.expected_hex) }.to_bytes();
        write_at(image, guard.rva, &decode_hex(hex)?)?;
    }
    Some(())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_anomaly_geometry_seed(
    image: *mut u8,
    size: u32,
    profile: &GuardProfile,
) -> bool {
    if image.is_null() || size != profile.image_size {
        return false;
    }
    // The fixture lends its initialized allocation exclusively for this call.
    seed(
        unsafe { std::slice::from_raw_parts_mut(image, size as usize) },
        profile,
    )
    .is_some()
}
