// SPDX-License-Identifier: MIT
//! Resolves the selected build's Rust-owned geometry and native profile facts.
//! The native leaf passes only the build index and the mapped module.

use super::Layout;
use crate::engine_guards::{NativeSite, UniqueGuard};
use crate::native_geometry_abi::Engine;

/// Static tables and engine globals of one build.
pub(super) struct Facts {
    pub(super) sites: &'static [NativeSite],
    pub(super) layout_sites: &'static [NativeSite],
    pub(super) unique: &'static [UniqueGuard],
    pub(super) hierarchy_vtables: [u32; 4],
    pub(super) vertex_constant_buffers: u32,
}

#[cfg(windows)]
pub(super) fn geometry(engine: Engine) -> Option<Facts> {
    let profile = crate::geometry_profile::PROFILES.get(engine.build)?;
    Some(Facts {
        sites: &profile.sites,
        layout_sites: &profile.layout_sites,
        unique: &profile.unique,
        hierarchy_vtables: profile.hierarchy_vtables,
        vertex_constant_buffers: profile.vertex_constant_buffers_rva,
    })
}

#[cfg(windows)]
pub(super) fn image_size(engine: Engine) -> Option<u32> {
    Some(
        crate::native_profile::PROFILES
            .get(engine.build)?
            .image_size,
    )
}

#[cfg(windows)]
pub(super) fn layout(engine: Engine) -> Option<Layout> {
    let geometry = crate::geometry_profile::PROFILES.get(engine.build)?;
    Some(Layout {
        phase: geometry.render_phase_rva,
        viewport: geometry.second_viewport_rva,
        frame: crate::native_profile::PROFILES
            .get(engine.build)?
            .device_frame_rva,
        world: geometry.world_matrix_rva,
    })
}

// Only Windows builds carry the engine profiles. Other targets exercise the
// pure palette and registry contracts and refuse every engine admission.
#[cfg(not(windows))]
pub(super) fn geometry(_: Engine) -> Option<Facts> {
    None
}

#[cfg(not(windows))]
pub(super) fn image_size(_: Engine) -> Option<u32> {
    None
}

#[cfg(not(windows))]
pub(super) fn layout(_: Engine) -> Option<Layout> {
    None
}
