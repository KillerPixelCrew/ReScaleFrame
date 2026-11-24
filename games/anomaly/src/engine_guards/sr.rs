// SPDX-License-Identifier: MIT
use super::{Image, guard_first_person};
use crate::sr_profile::{
    Profile, SITE_PHASE_BLUR, SITE_PHASE_COMBINE, SITE_PHASE_SCENE_PREPARE, SITE_PHASE_SUNSHAFTS,
    SITE_POSITION_SETUP, SITE_RM_NEAR, SITE_SET_MIP_LOD_BIAS, SITE_SET_P, SITE_SUN_CASCADES,
};
use std::ffi::c_char;
use std::ptr;

/// The first failing guard of the selected build's SR stage, or null when every guard matches. The returned name is
/// borrowed from the static profile records or the static SSAO name.
pub(crate) fn sr_guard_failure(
    image: &Image,
    profile: &Profile,
    world_window: bool,
    temporal: bool,
) -> *const c_char {
    let sites = &profile.function_sites;
    let set_projection = &sites[SITE_SET_P];
    let set_mip_bias = &sites[SITE_SET_MIP_LOD_BIAS];
    let blur = &sites[SITE_PHASE_BLUR];
    let sun_cascades = &sites[SITE_SUN_CASCADES];
    // Stage 2 writes the static viewport. Its helper code encodes both data addresses.
    if world_window
        && !unsafe {
            image.matches_hex_guard(
                profile.ssao_set_viewport_rva,
                profile.ssao_set_viewport_code.as_ptr(),
            )
        }
    {
        return c"set_viewport (r4_rendertarget_phase_ssao.cpp:3)".as_ptr();
    }
    let unique = profile.unique_guards;
    if temporal {
        // Stage 3 calls set_P and SetMipLODBias and requires all six rmNear producers.
        for guard in unique {
            if guard.rva == set_projection.rva
                && !unsafe { image.matches_hex_guard(guard.rva, guard.expected_hex) }
            {
                return set_projection.name;
            }
        }
        if !image.matches_site(set_mip_bias.rva, &set_mip_bias.expected) {
            return set_mip_bias.name;
        }
        for site in &profile.first_person_sites {
            if !guard_first_person(
                image,
                site.set_p_call,
                site.rm_near_call,
                set_projection.rva,
                sites[SITE_RM_NEAR].rva,
            ) {
                return site.name.as_ptr();
            }
        }
        for site in &profile.reconstruction_guards {
            if !unsafe { image.matches_hex_guard(site.rva, site.expected_hex) } {
                return site.name;
            }
        }
    }

    // The geometry producer already verified and installed the shared rmNear slot.
    for index in [
        SITE_PHASE_SCENE_PREPARE,
        SITE_PHASE_COMBINE,
        SITE_PHASE_SUNSHAFTS,
        SITE_PHASE_BLUR,
        SITE_SUN_CASCADES,
        SITE_POSITION_SETUP,
    ] {
        let site = &sites[index];
        if !image.matches_site(site.rva, &site.expected) {
            return site.name;
        }
    }
    for site in unique {
        if (site.rva == blur.rva || site.rva == sun_cascades.rva)
            && !unsafe { image.matches_hex_guard(site.rva, site.expected_hex) }
        {
            return site.name;
        }
    }
    for site in &profile.call_sites {
        if !image.matches_site(site.rva, &site.expected) {
            return site.name;
        }
    }
    ptr::null()
}
