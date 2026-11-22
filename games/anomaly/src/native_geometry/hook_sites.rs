// SPDX-License-Identifier: MIT
use crate::engine_guards::NativeSite;
use crate::native_hooks::Site;
use std::ffi::c_void;

const _: () = {
    assert!(size_of::<Site>() == 48);
    assert!(std::mem::offset_of!(Site, expected) == 12);
    assert!(std::mem::offset_of!(Site, replacement) == 32);
    assert!(std::mem::offset_of!(Site, original) == 40);
};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_build_hook_sites(
    profile: *const NativeSite,
    replacements: *const *mut c_void,
    originals: *mut *mut c_void,
    sites: *mut Site,
    count: usize,
) {
    // The native geometry owner lends four arrays of the same length. HookSet
    // uses the native original slots after this call, so store their addresses.
    let profile = unsafe { std::slice::from_raw_parts(profile, count) };
    let replacements = unsafe { std::slice::from_raw_parts(replacements, count) };
    for (i, (site, &replacement)) in profile.iter().zip(replacements).enumerate() {
        unsafe {
            sites.add(i).write(Site {
                _name: site.name,
                rva: site.rva,
                expected: site.expected,
                replacement,
                original: originals.add(i),
            });
        }
    }
}
