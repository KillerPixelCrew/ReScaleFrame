// SPDX-License-Identifier: MIT
// Public host records come from the generated SDK bindings.
pub(crate) use rsf_game_sys::*;
use std::mem::{offset_of, size_of};

pub(super) const DUMP_RAW: u32 = 0;
pub(super) const DUMP_HDR: u32 = 3;
pub(super) const DUMP_DEPTH: u32 = 4;
pub(super) const DUMP_DENSE_MOTION: u32 = 7;

pub(crate) type Pass = rsf_game_render_pass;
pub(crate) type Surface = rsf_d3d11_auxiliary_surface;

const _: () = {
    assert!(size_of::<rsf_d3d11_host_setup>() == 168);
    assert!(offset_of!(rsf_d3d11_host_setup, engine_type) == 48);
    assert!(offset_of!(rsf_d3d11_host_setup, units_to_meters) == 72);
    assert!(offset_of!(rsf_d3d11_host_setup, motion) == 80);
    assert!(offset_of!(rsf_d3d11_host_setup, quality_choices) == 128);
    assert!(offset_of!(rsf_d3d11_host_setup, accept_window) == 136);
    assert!(offset_of!(rsf_d3d11_host_setup, capabilities) == 152);
    assert!(offset_of!(rsf_d3d11_host_setup, depth_far_plane) == 160);
    assert!(size_of::<rsf_d3d11_present_dispatch>() == 32);
    assert!(offset_of!(rsf_d3d11_present_dispatch, native_result) == 28);
    assert!(size_of::<rsf_d3d11_chain_dispatch>() == 72);
    assert!(offset_of!(rsf_d3d11_chain_dispatch, receiver) == 16);
    assert!(offset_of!(rsf_d3d11_chain_dispatch, args) == 24);
    assert!(offset_of!(rsf_d3d11_chain_dispatch, result) == 64);
    assert!(size_of::<rsf_d3d11_native_chain>() == 32);
    assert!(size_of::<rsf_d3d11_creation_scope>() == 24);
    assert!(size_of::<rsf_d3d11_presentation_binding>() == 16);
    assert!(size_of::<rsf_d3d11_capture_control>() == 16);
    assert!(size_of::<rsf_d3d11_host_capture>() == 64);
    assert!(offset_of!(rsf_d3d11_host_capture, view) == 56);
    assert!(size_of::<rsf_d3d11_auxiliary_pass>() == 32);
    assert!(offset_of!(rsf_d3d11_auxiliary_pass, surfaces) == 24);
    assert!(size_of::<rsf_d3d11_auxiliary_scope>() == 32);
    assert!(size_of::<rsf_d3d11_reduced_frame>() == 56);
    assert!(offset_of!(rsf_d3d11_reduced_frame, token) == 16);
    assert!(offset_of!(rsf_d3d11_reduced_frame, owner) == 24);
    assert!(offset_of!(rsf_d3d11_reduced_frame, surfaces) == 40);
    assert!(offset_of!(rsf_d3d11_reduced_frame, temporal_result) == 48);
    assert!(size_of::<rsf_d3d11_sampler_bias>() == 160);
    assert!(offset_of!(rsf_d3d11_sampler_bias, slot_mask) == 144);
    assert!(offset_of!(rsf_d3d11_sampler_bias, applied_mask) == 152);
};
