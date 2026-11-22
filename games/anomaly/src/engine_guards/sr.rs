// SPDX-License-Identifier: MIT
use super::{Image, NativeSite, UniqueGuard, rsf_anomaly_guard_first_person};
use crate::ffi::borrow_span;
use std::ffi::c_char;
use std::ptr;

#[repr(C)]
struct FirstPersonSite {
    name: *const c_char,
    _function_rva: u32,
    set_p_call: u32,
    rm_near_call: u32,
}

#[repr(C)]
struct Profile<'a> {
    scene_prepare: &'a NativeSite,
    combine: &'a NativeSite,
    sunshafts: &'a NativeSite,
    blur: &'a NativeSite,
    sun_cascades: &'a NativeSite,
    position_setup: &'a NativeSite,
    set_projection: &'a NativeSite,
    set_mip_bias: &'a NativeSite,
    unique: *const UniqueGuard,
    unique_count: usize,
    call_sites: &'a [NativeSite; 6],
    first_person: &'a [FirstPersonSite; 6],
    reconstruction: &'a [UniqueGuard; 4],
    ssao_code: *const c_char,
    ssao_rva: u32,
    rm_near_rva: u32,
}

const _: () = {
    assert!(size_of::<FirstPersonSite>() == 24);
    assert!(std::mem::offset_of!(FirstPersonSite, set_p_call) == 12);
    assert!(std::mem::offset_of!(FirstPersonSite, rm_near_call) == 16);
    assert!(size_of::<Profile>() == 120);
    assert!(std::mem::offset_of!(Profile, unique) == 64);
    assert!(std::mem::offset_of!(Profile, call_sites) == 80);
    assert!(std::mem::offset_of!(Profile, ssao_rva) == 112);
};

/// The facade lends the selected build's immutable records and terminated guard strings.
/// The returned site name is borrowed from the profile or the static SSAO name.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_guard_failure(
    image: &Image,
    profile: &Profile<'_>,
    world_window: bool,
    temporal: bool,
) -> *const c_char {
    // Stage 2 writes the static viewport. Its helper code encodes both data addresses.
    if world_window && !unsafe { image.matches_hex_guard(profile.ssao_rva, profile.ssao_code) } {
        return c"set_viewport (r4_rendertarget_phase_ssao.cpp:3)".as_ptr();
    }
    let unique = unsafe { borrow_span(profile.unique, profile.unique_count) };
    if temporal {
        // Stage 3 calls set_P and SetMipLODBias and requires all six rmNear producers.
        for guard in unique {
            if guard.rva == profile.set_projection.rva
                && !unsafe { image.matches_hex_guard(guard.rva, guard.expected_hex) }
            {
                return profile.set_projection.name;
            }
        }
        if !image.matches_site(profile.set_mip_bias.rva, &profile.set_mip_bias.expected) {
            return profile.set_mip_bias.name;
        }
        for site in profile.first_person {
            if !rsf_anomaly_guard_first_person(
                image,
                site.set_p_call,
                site.rm_near_call,
                profile.set_projection.rva,
                profile.rm_near_rva,
            ) {
                return site.name;
            }
        }
        for site in profile.reconstruction {
            if !unsafe { image.matches_hex_guard(site.rva, site.expected_hex) } {
                return site.name;
            }
        }
    }

    // The geometry producer already verified and installed the shared rmNear slot.
    for site in [
        profile.scene_prepare,
        profile.combine,
        profile.sunshafts,
        profile.blur,
        profile.sun_cascades,
        profile.position_setup,
    ] {
        if !image.matches_site(site.rva, &site.expected) {
            return site.name;
        }
    }
    for site in unique {
        if (site.rva == profile.blur.rva || site.rva == profile.sun_cascades.rva)
            && !unsafe { image.matches_hex_guard(site.rva, site.expected_hex) }
        {
            return site.name;
        }
    }
    for site in profile.call_sites {
        if !image.matches_site(site.rva, &site.expected) {
            return site.name;
        }
    }
    ptr::null()
}
