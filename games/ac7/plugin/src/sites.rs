// SPDX-License-Identifier: MIT
//! Admission of the exact-build code sites (rsf_ac7::sites) before any hook is made.
//!
//! native_renderer.rs keeps MinHook, the detours, the original pointers and the guarded code reads.
//! The renderer verifies post-patch depth gates installed by the carrier. The research for these
//! sites is in docs/research/ue418-hook-map.md and games/ac7/engine.json.
use crate::native_renderer::code_matches;
pub(crate) use rsf_ac7::sites::*;
use rsf_ac7::sys::{RSF_AC7_EVIDENCE_FIRST, RSF_AC7_EVIDENCE_LAST, RSF_AC7_HOOK_SITE_COUNT};

/// Exact-build admission in the former order: hooked entry sites, enlarged UI depth/view gates,
/// helper sites. The first difference is logged and refuses the build.
pub(crate) fn admit(base: *const u8, log: impl Fn(&str)) -> bool {
    if let Some(site) = HOOK_SITES
        .iter()
        .find(|site| !code_matches(base.wrapping_add(site.rva as usize), &site.expected))
    {
        log(&format!(
            "AC7 native renderer refused: expected entry bytes differ at RVA 0x{:x}",
            site.rva
        ));
        return false;
    }
    if let Some(gate) = DEPTH_GATES
        .iter()
        .find(|gate| !code_matches(base.wrapping_add(gate.rva as usize), gate.expected))
    {
        log(&format!(
            "AC7 native renderer refused: enlarged UI depth/view gate missing at RVA 0x{:x}",
            gate.rva
        ));
        return false;
    }
    if let Some(helper) = HELPERS
        .iter()
        .find(|helper| !code_matches(base.wrapping_add(helper.rva as usize), &helper.expected))
    {
        log(&format!(
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
