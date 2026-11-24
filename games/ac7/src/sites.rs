// SPDX-License-Identifier: MIT
//! The pass role of each Process hook and the separate-translucency depth gates the carrier
//! installs and the native renderer requires. Every site is discovered (discovery.rs); the
//! research is in docs/research/ue418-hook-map.md and games/ac7/engine.json.
use crate::discovery::{self, Site};
use crate::sys::{
    RSF_AC7_PROCESS_HOOKS, RSF_AC7_ROLE_AA, RSF_AC7_ROLE_COMPOSITE, RSF_AC7_ROLE_HUD,
    RSF_AC7_ROLE_MATERIAL, RSF_AC7_ROLE_OUTPUT, RSF_AC7_ROLE_TONEMAP,
};

/// The pass role of each Process site: tonemap, AA, material, HUD, composite, output.
pub const PROCESS_ROLES: [u32; RSF_AC7_PROCESS_HOOKS as usize] = [
    RSF_AC7_ROLE_TONEMAP,
    RSF_AC7_ROLE_AA,
    RSF_AC7_ROLE_MATERIAL,
    RSF_AC7_ROLE_HUD,
    RSF_AC7_ROLE_COMPOSITE,
    RSF_AC7_ROLE_OUTPUT,
];

/// An in-function branch or comparison that enlarged separate translucency depends on, at its
/// discovered RVA.
pub struct Gate {
    pub rva: usize,
    pub native: &'static [u8],
    pub expected: &'static [u8],
    pub description: &'static str,
}

const SHADER_DEPTH_NATIVE: &[u8] = b"\x0f\x2f\x86\x20\x02\x00\x00\x76\x04";
const SHADER_DEPTH_PATCHED: &[u8] = b"\x0f\x2f\x86\x20\x02\x00\x00\x74\x04";

/// The single depth gates: (site, native, patched, description). Each pattern fixes every byte
/// here except the patched one.
#[rustfmt::skip]
const SINGLE_GATES: [(Site, &[u8], &[u8], &str); 5] = [
    (Site::GateDepthFill, b"\x76\x0e", b"\x74\x0e", "the depth allocation and fill"),
    (Site::GateDepthBind, b"\x76\x17", b"\x74\x17", "the depth bind"),
    (Site::GateSnapshotResolve, b"\x44\x0f\x47\xf9", b"\x44\x0f\x45\xf9", "the snapshot resolve"),
    (Site::GateResolve, b"\x76\x17", b"\x74\x17", "the resolve"),
    (Site::GateLayerView, b"\x0f\x2f\x80\x20\x02\x00\x00\x76\x0e",
        b"\x0f\x2f\x80\x20\x02\x00\x00\x74\x0e", "the layer view selection"),
];

/// Every depth gate at its discovered site: the five single gates, then each shader-depth
/// selection. The error names the site that did not resolve.
pub fn depth_gates() -> Result<Vec<Gate>, String> {
    discovery::admit(SINGLE_GATES.iter().map(|&(site, ..)| site as usize))?;
    let shader = discovery::admit_shader_depth()?;
    let single = SINGLE_GATES
        .iter()
        .map(|&(site, native, expected, description)| Gate {
            rva: discovery::rva(site as usize),
            native,
            expected,
            description,
        });
    let shader = shader.iter().map(|&rva| Gate {
        rva,
        native: SHADER_DEPTH_NATIVE,
        expected: SHADER_DEPTH_PATCHED,
        description: "a shader depth selection",
    });
    Ok(single.chain(shader).collect())
}
