// SPDX-License-Identifier: MIT
use super::profile;
use crate::native_geometry_abi::Engine;
use crate::native_hooks::Site;
use std::ffi::c_void;

/// Pairs each profiled detour entry of `build` with the native replacement and
/// the address of the native original slot. The hook set uses those slots after
/// this call. Refuses an unknown build or a table of the wrong length.
///
/// # Safety
/// `replacements` and `originals` name `count` live entries, and `sites` names
/// `count` writable records.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_build_hook_sites(
    build: usize,
    replacements: *const *mut c_void,
    originals: *mut *mut c_void,
    sites: *mut Site,
    count: usize,
) -> bool {
    let Some(profile) = profile::geometry(Engine { module: 0, build }) else {
        return false;
    };
    if profile.sites.len() != count {
        return false;
    }
    let replacements = unsafe { std::slice::from_raw_parts(replacements, count) };
    for (i, (site, &replacement)) in profile.sites.iter().zip(replacements).enumerate() {
        unsafe {
            sites.add(i).write(Site {
                name: site.name,
                rva: site.rva,
                expected: site.expected,
                replacement,
                original: originals.add(i),
            });
        }
    }
    true
}

/// The profiled SizeOfImage of uild, or 0 for an unknown build.
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_image_size(build: usize) -> u32 {
    profile::image_size(Engine { module: 0, build }).unwrap_or(0)
}
