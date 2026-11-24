// SPDX-License-Identifier: MIT
//! Admission of the discovered code sites (rsf_ac7::discovery) before any hook is made.
//!
//! native_renderer.rs keeps MinHook, the detours, the original pointers and the guarded code reads.
//! The renderer verifies post-patch depth gates installed by the carrier. The research for these
//! sites is in docs/research/ue418-hook-map.md and games/ac7/engine.json.
use crate::native_renderer::code_matches;
use rsf_ac7::discovery::{self, HOOKS, Site};
use rsf_ac7::sites::{PROCESS_ROLES, depth_gates};
use rsf_ac7::sys::{RSF_AC7_EVIDENCE_FIRST, RSF_AC7_EVIDENCE_LAST, RSF_AC7_HOOK_SITE_COUNT};

/// Admission in the former order: every hook, helper and global discovered and its code still
/// matching in memory, the enlarged UI depth/view gates as the carrier patched them, and the
/// renderer's own patch sites. The first refusal is logged and names its site.
pub(crate) fn admit(base: *const u8, log: impl Fn(&str)) -> bool {
    let patches = [
        Site::VelocityRender,
        Site::VelocitySize,
        Site::VelocityMoved,
        Site::ScenePrecision,
    ];
    let sites = (0..Site::AaPassVtable as usize).chain(patches.map(usize::from));
    if let Err(reason) = discovery::admit(sites) {
        log(&format!("AC7 native renderer refused: {reason}"));
        return false;
    }
    log(&discovery::summary());
    // Globals have no code to match; every hook and helper is matched again in memory.
    if let Some(site) =
        (0..Site::Engine as usize).find(|&site| !discovery::verify(base.addr(), site))
    {
        log(&format!(
            "AC7 native renderer refused: site '{}' bytes differ at RVA 0x{:x}",
            discovery::name(site),
            discovery::rva(site)
        ));
        return false;
    }
    let gates = match depth_gates() {
        Ok(gates) => gates,
        Err(reason) => {
            log(&format!("AC7 native renderer refused: {reason}"));
            return false;
        }
    };
    if let Some(gate) = gates
        .iter()
        .find(|gate| !code_matches(base.wrapping_add(gate.rva), gate.expected))
    {
        log(&format!(
            "AC7 native renderer refused: enlarged UI depth/view gate missing at RVA 0x{:x}",
            gate.rva
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

/// The discovered RVA of `hook`; zero until admitted and for an index past the hooks.
#[cfg_attr(feature = "warp-fixture", unsafe(no_mangle))]
pub(crate) extern "C" fn rsf_ac7_hook_rva(hook: u32) -> u32 {
    let hook = hook as usize;
    if hook < HOOKS {
        discovery::rva(hook) as u32
    } else {
        0
    }
}

#[cfg_attr(feature = "warp-fixture", unsafe(no_mangle))]
pub(crate) extern "C" fn rsf_ac7_hook_role(hook: u32) -> u32 {
    PROCESS_ROLES.get(hook as usize).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests;
