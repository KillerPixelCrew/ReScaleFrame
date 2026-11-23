// SPDX-License-Identifier: MIT
//! Admission of the exact-build code sites (rsf_ac7::sites) before any hook is made, and the code
//! access the renderer, the patches and TrueSky share.
//!
//! native_renderer.rs keeps MinHook, the detours and the original pointers, and lends the guarded
//! reads and protected code writes through `rsf_ac7_code_leaves`.
//! The renderer verifies post-patch depth gates installed by the carrier. The research for these
//! sites is in docs/research/ue418-hook-map.md and games/ac7/engine.json.
pub(crate) use rsf_ac7::sites::*;
use rsf_ac7::sys::{
    RSF_AC7_EVIDENCE_FIRST, RSF_AC7_EVIDENCE_LAST, RSF_AC7_HOOK_SITE_COUNT, rsf_ac7_code_leaves,
};
use std::ffi::CString;

/// Read-only access needed for exact-build admission.
pub(crate) trait CodeView {
    fn matches(&self, address: *const u8, expected: &[u8]) -> bool;
    fn log(&self, message: &str);
}

/// A writer replaces verified code, leaving the code unchanged on refusal.
pub(crate) trait Code: CodeView {
    fn write(&self, site: *mut u8, expected: &[u8], replacement: &[u8]) -> bool;
}

type CompareFn = unsafe extern "C" fn(*const u8, *const u8, usize) -> bool;
type WriteFn = unsafe extern "C" fn(*mut u8, *const u8, *const u8, usize) -> bool;

pub(crate) struct NativeCodeView {
    compare: CompareFn,
    pub(crate) log: rsf_game_sys::rsf_game_log_fn,
    pub(crate) user: *mut std::ffi::c_void,
}

impl NativeCodeView {
    fn new(leaves: &rsf_ac7_code_leaves) -> Option<Self> {
        Some(Self {
            compare: leaves.matches?,
            log: leaves.log,
            user: leaves.user,
        })
    }
}

impl CodeView for NativeCodeView {
    fn matches(&self, address: *const u8, expected: &[u8]) -> bool {
        // The native leaf guards access to the live image.
        unsafe { (self.compare)(address, expected.as_ptr(), expected.len()) }
    }

    fn log(&self, message: &str) {
        if let (Some(log), Ok(text)) = (self.log, CString::new(message)) {
            unsafe { log(self.user, text.as_ptr()) };
        }
    }
}

pub(crate) struct NativeCode {
    pub(crate) view: NativeCodeView,
    write: WriteFn,
}

impl NativeCode {
    /// # Safety
    /// The table, user and callbacks must be valid for all uses of the returned code access.
    pub(crate) unsafe fn new(leaves: *const rsf_ac7_code_leaves) -> Option<Self> {
        let leaves = unsafe { leaves.as_ref() }?;
        Some(Self {
            view: NativeCodeView::new(leaves)?,
            write: leaves.write?,
        })
    }
}

impl CodeView for NativeCode {
    fn matches(&self, address: *const u8, expected: &[u8]) -> bool {
        self.view.matches(address, expected)
    }

    fn log(&self, message: &str) {
        self.view.log(message);
    }
}

impl Code for NativeCode {
    fn write(&self, site: *mut u8, expected: &[u8], replacement: &[u8]) -> bool {
        if expected.len() != replacement.len() {
            return false;
        }
        // The native leaf writes only after checking the live bytes.
        unsafe {
            (self.write)(
                site,
                expected.as_ptr(),
                replacement.as_ptr(),
                expected.len(),
            )
        }
    }
}

/// Code at `rva` in the module at `base`. The address is only passed to the native leaves.
pub(crate) fn at(base: *const u8, rva: u32) -> *const u8 {
    base.wrapping_add(rva as usize)
}

/// Exact-build admission in the former order: hooked entry sites, enlarged UI depth/view gates,
/// helper sites. The first difference is logged and refuses the build.
pub(crate) fn admit(base: *const u8, code: &impl CodeView) -> bool {
    if let Some(site) = HOOK_SITES
        .iter()
        .find(|site| !code.matches(at(base, site.rva), &site.expected))
    {
        code.log(&format!(
            "AC7 native renderer refused: expected entry bytes differ at RVA 0x{:x}",
            site.rva
        ));
        return false;
    }
    if let Some(gate) = DEPTH_GATES
        .iter()
        .find(|gate| !code.matches(at(base, gate.rva), gate.expected))
    {
        code.log(&format!(
            "AC7 native renderer refused: enlarged UI depth/view gate missing at RVA 0x{:x}",
            gate.rva
        ));
        return false;
    }
    if let Some(helper) = HELPERS
        .iter()
        .find(|helper| !code.matches(at(base, helper.rva), &helper.expected))
    {
        code.log(&format!(
            "AC7 native renderer refused: expected helper bytes differ at RVA 0x{:x}",
            helper.rva
        ));
        return false;
    }
    true
}

/// Evidence hooks run for every pixel uniform bind of every draw. Their sites are still verified,
/// but they are hooked only when the pixel binding diagnostic is asked for.
///
/// The three site queries are C symbols only in warp-fixture builds: the WARP view fixture
/// (tests/ac7_view_fixture.rs) is built into rsf-ac7, below this crate, and checks the table by name.
#[cfg_attr(feature = "warp-fixture", unsafe(export_name = "rsf_ac7_hook_wanted"))]
pub(crate) extern "C" fn wanted(hook: u32, diagnostics: bool) -> bool {
    hook < RSF_AC7_HOOK_SITE_COUNT
        && (diagnostics || !(RSF_AC7_EVIDENCE_FIRST..=RSF_AC7_EVIDENCE_LAST).contains(&hook))
}

#[cfg_attr(feature = "warp-fixture", unsafe(no_mangle))]
pub(crate) extern "C" fn rsf_ac7_hook_rva(hook: u32) -> u32 {
    HOOK_SITES.get(hook as usize).map_or(0, |site| site.rva)
}

#[cfg_attr(feature = "warp-fixture", unsafe(no_mangle))]
pub(crate) extern "C" fn rsf_ac7_hook_role(hook: u32) -> u32 {
    PROCESS_ROLES.get(hook as usize).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests;
