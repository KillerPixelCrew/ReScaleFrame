// SPDX-License-Identifier: MIT
//! Engine code patches the native renderer applies at activation and restores at retirement:
//! scene-colour precision (+1095620) and the temporal velocity group (+118A4E3, +10FDD0C,
//! +11838C4). The ownership record lives in the native renderer (`rsf_ac7_code_patches`); only
//! this module changes it. Writes go through the native code leaf, which checks the expected
//! bytes again under code protection.
//!
//! The TrueSky plugin patches and call relays (TrueSkyPluginRender_MT.dll) are truesky.rs; their
//! device-dependent readiness stays in native_renderer.cpp.
use crate::sites::{Code, NativeCode};
use crate::sites_abi::{
    RSF_AC7_VELOCITY_PATCHES, rsf_ac7_code_leaves, rsf_ac7_code_patch, rsf_ac7_code_patches,
};
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

    fn descriptor(&self) -> rsf_ac7_code_patch {
        let mut out = rsf_ac7_code_patch {
            rva: self.rva,
            size: self.native.len() as u32,
            native: [0; 16],
            replacement: [0; 16],
        };
        out.native[..self.native.len()].copy_from_slice(self.native);
        out.replacement[..self.replacement.len()].copy_from_slice(self.replacement);
        out
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
    code: &impl Code,
) -> bool {
    if state.velocity_applied != 0 {
        return state.velocity_applied as usize == VELOCITY.len();
    }
    if let Some(patch) = VELOCITY
        .iter()
        .find(|patch| !code.matches(base.wrapping_add(patch.rva as usize), patch.native))
    {
        code.log(&format!(
            "AC7 temporal velocity refused: expected bytes differ at RVA 0x{:x}",
            patch.rva
        ));
        return false;
    }
    state.velocity_module = base;
    for patch in &VELOCITY {
        if !code.write(
            base.wrapping_add(patch.rva as usize),
            patch.native,
            patch.replacement,
        ) {
            if !velocity_restore(state, code) {
                code.log("AC7 temporal velocity rollback refused; patched module retained");
            }
            return false;
        }
        state.velocity_applied += 1;
    }
    code.log(
        "AC7 temporal velocity: native pass enabled without TAA; small and fixed-root movable opaque geometry retained with native history, camera-cut and shader checks",
    );
    true
}

/// Restores in reverse order. A failed write keeps that patch and those before it owned.
pub(crate) fn velocity_restore(state: &mut rsf_ac7_code_patches, code: &impl Code) -> bool {
    while state.velocity_applied != 0 {
        let Some(patch) = VELOCITY.get(state.velocity_applied as usize - 1) else {
            return false;
        };
        if !code.write(
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
pub(crate) fn scene_precision_apply(
    state: &mut rsf_ac7_code_patches,
    base: *mut u8,
    code: &impl Code,
) -> bool {
    if !state.scene_precision_site.is_null() {
        return true;
    }
    let site = base.wrapping_add(SCENE_PRECISION_RVA as usize);
    if !code.matches(site, &SCENE_PRECISION_EXPECTED)
        || !code.write(site, &NATIVE_SCENE_FORMAT, &PRECISE_SCENE_FORMAT)
    {
        code.log(
            "native scene-precision patch refused: expected bytes differ or code protection failed at RVA 0x1095620",
        );
        return false;
    }
    state.scene_precision_site = site;
    code.log(
        "native scene precision: allocation format policy 4 at RVA 0x1095620; engine owns FP16 resource rebuild",
    );
    true
}

pub(crate) fn scene_precision_restore(state: &mut rsf_ac7_code_patches, code: &impl Code) -> bool {
    if state.scene_precision_site.is_null() {
        return true;
    }
    if !code.write(
        state.scene_precision_site,
        &PRECISE_SCENE_FORMAT,
        &NATIVE_SCENE_FORMAT,
    ) {
        code.log("native scene-precision patch could not restore; renderer retained");
        return false;
    }
    state.scene_precision_site = null_mut();
    true
}

/// Activation order of the former start: scene precision, then temporal velocity.
pub(crate) fn apply(state: &mut rsf_ac7_code_patches, base: *mut u8, code: &impl Code) -> bool {
    scene_precision_apply(state, base, code) && velocity_apply(state, base, code)
}

/// Retirement order of the former stop: temporal velocity, then scene precision.
pub(crate) fn restore(state: &mut rsf_ac7_code_patches, code: &impl Code) -> bool {
    if !velocity_restore(state, code) {
        code.log("AC7 temporal velocity patch could not restore; renderer retained");
        return false;
    }
    scene_precision_restore(state, code)
}

/// The ownership record and leaves of one native call.
///
/// # Safety
/// `patches` and `leaves` are null or valid for the call, and nothing else uses `patches` during
/// it.
unsafe fn native<'a>(
    patches: *mut rsf_ac7_code_patches,
    leaves: *const rsf_ac7_code_leaves,
) -> Option<(&'a mut rsf_ac7_code_patches, NativeCode<'a>)> {
    let patches = unsafe { patches.as_mut() }?;
    let code = unsafe { NativeCode::new(leaves) }?;
    Some((patches, code))
}

/// # Safety
/// `out` is null or writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_patch(index: u32, out: *mut rsf_ac7_code_patch) -> bool {
    match (VELOCITY.get(index as usize), unsafe { out.as_mut() }) {
        (Some(patch), Some(out)) => {
            *out = patch.descriptor();
            true
        }
        _ => false,
    }
}

/// # Safety
/// See `native`. The leaves receive `base` plus each patch RVA.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_apply(
    patches: *mut rsf_ac7_code_patches,
    base: *mut u8,
    leaves: *const rsf_ac7_code_leaves,
) -> bool {
    unsafe { native(patches, leaves) }
        .is_some_and(|(patches, code)| velocity_apply(patches, base, &code))
}

/// # Safety
/// See `native`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_restore(
    patches: *mut rsf_ac7_code_patches,
    leaves: *const rsf_ac7_code_leaves,
) -> bool {
    unsafe { native(patches, leaves) }
        .is_some_and(|(patches, code)| velocity_restore(patches, &code))
}

/// # Safety
/// See `native`. The leaves receive `base` plus each patch RVA.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_engine_patches_apply(
    patches: *mut rsf_ac7_code_patches,
    base: *mut u8,
    leaves: *const rsf_ac7_code_leaves,
) -> bool {
    unsafe { native(patches, leaves) }.is_some_and(|(patches, code)| apply(patches, base, &code))
}

/// # Safety
/// See `native`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_engine_patches_restore(
    patches: *mut rsf_ac7_code_patches,
    leaves: *const rsf_ac7_code_leaves,
) -> bool {
    unsafe { native(patches, leaves) }.is_some_and(|(patches, code)| restore(patches, &code))
}

#[cfg(test)]
mod tests;
