// SPDX-License-Identifier: MIT
//! Engine code patches the native renderer applies at activation and restores at retirement:
//! scene-colour precision (+1095620) and the temporal velocity group (+118A4E3, +10FDD0C,
//! +11838C4). The ownership record lives in the native renderer (`rsf_ac7_code_patches`); only
//! this module changes it. `write_code` checks the expected bytes again under code protection.
//!
//! The TrueSky plugin patches and call relays (TrueSkyPluginRender_MT.dll) are truesky.rs; their
//! device-dependent readiness stays in native_renderer/sky.rs.
use crate::native_renderer::{code_matches, write_code};
use crate::sys::{RSF_AC7_VELOCITY_PATCHES, rsf_ac7_code_patches};
use std::ptr::null_mut;

/// Replaces `native` with `replacement` at `rva`.
pub(crate) struct Patch {
    pub(crate) rva: u32,
    pub(crate) native: &'static [u8],
    pub(crate) replacement: &'static [u8],
}

impl Patch {
    const fn new<const N: usize>(
        rva: u32,
        native: &'static [u8; N],
        replacement: &'static [u8; N],
    ) -> Self {
        Self {
            rva,
            native,
            replacement,
        }
    }
}

/// UE4's motion-blur selection is too restrictive for temporal reconstruction. Keep visibility,
/// main-pass relevance, mobility, camera cuts, native transform/bone history and shader support.
/// The transform test becomes unconditional only after a successful history lookup.
pub(crate) const VELOCITY: [Patch; RSF_AC7_VELOCITY_PATCHES as usize] = [
    // Produce velocity even without native TAA/motion blur.
    Patch::new(0x118a4e3, b"\x75\x0e", b"\x90\x90"),
    // Small weapons and distant vehicles.
    Patch::new(0x10fdd0c, b"\x76\x38", b"\x90\x90"),
    // Bone/WPO motion with unchanged root transform.
    Patch::new(0x11838c4, b"\x77\x88", b"\xeb\x88"),
];

/// Scene-colour allocation format policy: the load `mov rax,[rip+..]; mov rcx,rbx;
/// mov eax,[rax+4]` becomes `mov eax,4` with the register move kept (format policy 4).
pub(crate) const SCENE_PRECISION_RVA: u32 = 0x1095620;
pub(crate) const SCENE_PRECISION_EXPECTED: [u8; 16] =
    *b"\x48\x8b\x05\x29\x33\xbc\x02\x48\x8b\xcb\x8b\x40\x04\x89\x45\xd7";
pub(crate) const NATIVE_SCENE_FORMAT: [u8; 13] =
    *b"\x48\x8b\x05\x29\x33\xbc\x02\x48\x8b\xcb\x8b\x40\x04";
pub(crate) const PRECISE_SCENE_FORMAT: [u8; 13] =
    *b"\xb8\x04\x00\x00\x00\x90\x90\x48\x8b\xcb\x90\x90\x90";

/// All-or-nothing: every site is checked before the first write, and a failed write rolls the
/// group back. An owned group answers whether it is complete.
pub(crate) fn velocity_apply(
    state: &mut rsf_ac7_code_patches,
    base: *mut u8,
    log: &impl Fn(&str),
) -> bool {
    if state.velocity_applied != 0 {
        return state.velocity_applied as usize == VELOCITY.len();
    }
    if let Some(patch) = VELOCITY
        .iter()
        .find(|patch| !code_matches(base.wrapping_add(patch.rva as usize), patch.native))
    {
        log(&format!(
            "AC7 temporal velocity refused: expected bytes differ at RVA 0x{:x}",
            patch.rva
        ));
        return false;
    }
    state.velocity_module = base;
    for patch in &VELOCITY {
        if !write_code(
            base.wrapping_add(patch.rva as usize),
            patch.native,
            patch.replacement,
        ) {
            if !velocity_restore(state) {
                log("AC7 temporal velocity rollback refused; patched module retained");
            }
            return false;
        }
        state.velocity_applied += 1;
    }
    log(
        "AC7 temporal velocity: native pass enabled without TAA; small and fixed-root movable opaque geometry retained with native history, camera-cut and shader checks",
    );
    true
}

/// Restores in reverse order. A failed write keeps that patch and those before it owned.
pub(crate) fn velocity_restore(state: &mut rsf_ac7_code_patches) -> bool {
    while state.velocity_applied != 0 {
        let Some(patch) = VELOCITY.get(state.velocity_applied as usize - 1) else {
            return false;
        };
        if !write_code(
            state.velocity_module.wrapping_add(patch.rva as usize),
            patch.replacement,
            patch.native,
        ) {
            return false;
        }
        state.velocity_applied -= 1;
    }
    state.velocity_module = null_mut();
    true
}

/// Allocate compares the effective format to its cache before UpdateRHI, so existing packed
/// targets retire through the engine's own reallocation path on the next family.
fn scene_precision_apply(
    state: &mut rsf_ac7_code_patches,
    base: *mut u8,
    log: &impl Fn(&str),
) -> bool {
    if !state.scene_precision_site.is_null() {
        return true;
    }
    let site = base.wrapping_add(SCENE_PRECISION_RVA as usize);
    if !code_matches(site, &SCENE_PRECISION_EXPECTED)
        || !write_code(site, &NATIVE_SCENE_FORMAT, &PRECISE_SCENE_FORMAT)
    {
        log(
            "native scene-precision patch refused: expected bytes differ or code protection failed at RVA 0x1095620",
        );
        return false;
    }
    state.scene_precision_site = site;
    log(
        "native scene precision: allocation format policy 4 at RVA 0x1095620; engine owns FP16 resource rebuild",
    );
    true
}

fn scene_precision_restore(state: &mut rsf_ac7_code_patches, log: &impl Fn(&str)) -> bool {
    if state.scene_precision_site.is_null() {
        return true;
    }
    if !write_code(
        state.scene_precision_site,
        &PRECISE_SCENE_FORMAT,
        &NATIVE_SCENE_FORMAT,
    ) {
        log("native scene-precision patch could not restore; renderer retained");
        return false;
    }
    state.scene_precision_site = null_mut();
    true
}

/// Activation order of the former start: scene precision, then temporal velocity.
pub(crate) fn apply(state: &mut rsf_ac7_code_patches, base: *mut u8, log: impl Fn(&str)) -> bool {
    scene_precision_apply(state, base, &log) && velocity_apply(state, base, &log)
}

/// Retirement order of the former stop: temporal velocity, then scene precision.
pub(crate) fn restore(state: &mut rsf_ac7_code_patches, log: impl Fn(&str)) -> bool {
    if !velocity_restore(state) {
        log("AC7 temporal velocity patch could not restore; renderer retained");
        return false;
    }
    scene_precision_restore(state, &log)
}

/// The C entries of the native graph fixture (tests/ac7_view_fixture.rs, through ac7_sites.h).
/// They log nowhere.
#[cfg(feature = "warp-fixture")]
mod fixture {
    use super::{VELOCITY, velocity_apply, velocity_restore};
    use crate::sys::{rsf_ac7_code_patch, rsf_ac7_code_patches};

    /// # Safety
    /// `out` is null or writable.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn rsf_ac7_velocity_patch(index: u32, out: *mut rsf_ac7_code_patch) -> bool {
        let (Some(patch), Some(out)) = (VELOCITY.get(index as usize), unsafe { out.as_mut() })
        else {
            return false;
        };
        *out = rsf_ac7_code_patch {
            rva: patch.rva,
            size: patch.native.len() as u32,
            native: [0; 16],
            replacement: [0; 16],
        };
        out.native[..patch.native.len()].copy_from_slice(patch.native);
        out.replacement[..patch.replacement.len()].copy_from_slice(patch.replacement);
        true
    }

    /// # Safety
    /// `patches` is null or valid for the call and used by nothing else during it; `base` plus
    /// each patch RVA is the borrowed fixture image.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn rsf_ac7_velocity_apply(
        patches: *mut rsf_ac7_code_patches,
        base: *mut u8,
    ) -> bool {
        unsafe { patches.as_mut() }.is_some_and(|patches| velocity_apply(patches, base, &|_| {}))
    }

    /// # Safety
    /// As `rsf_ac7_velocity_apply`.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn rsf_ac7_velocity_restore(patches: *mut rsf_ac7_code_patches) -> bool {
        unsafe { patches.as_mut() }.is_some_and(velocity_restore)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_the_former_patches() {
        let rvas: Vec<u32> = VELOCITY.iter().map(|patch| patch.rva).collect();
        assert_eq!(rvas, [0x118a4e3, 0x10fdd0c, 0x11838c4]);
        assert!(
            VELOCITY
                .iter()
                .all(|patch| patch.native.len() == 2 && patch.replacement.len() == 2)
        );
        assert_eq!(NATIVE_SCENE_FORMAT, SCENE_PRECISION_EXPECTED[..13]);
    }
}
