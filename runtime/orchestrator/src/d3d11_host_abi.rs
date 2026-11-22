// SPDX-License-Identifier: MIT
// Public host records come from the generated SDK bindings.
#![allow(non_camel_case_types)]

use rsf_game_sys::rsf_d3d11_auxiliary_surface;
use rsf_game_sys::rsf_game_render_pass;
pub(crate) use rsf_game_sys::*;
pub(crate) use rsf_game_sys::{
    RSF_D3D11_CAPTURE_CONTROL_VERSION, RSF_D3D11_CAPTURE_FAILURES, RSF_D3D11_CAPTURE_SEQUENCE,
    RSF_D3D11_HOST_CAP_FG_INPUTS, RSF_D3D11_HOST_CAP_INPUT_TIMELINE,
    RSF_D3D11_HOST_CAP_RENDERED_SOURCE, rsf_d3d11_auxiliary_pass, rsf_d3d11_auxiliary_scope,
    rsf_d3d11_capture_control, rsf_d3d11_creation_scope, rsf_d3d11_host_capture,
    rsf_d3d11_host_setup, rsf_d3d11_native_chain, rsf_d3d11_present_dispatch,
    rsf_d3d11_presentation_binding, rsf_d3d11_sampler_bias,
};
use std::mem::{offset_of, size_of};

pub(crate) use rsf_game_sys::{
    RSF_D3D11_REDUCED_CANCEL, RSF_D3D11_REDUCED_CANCELLED, RSF_D3D11_REDUCED_CLEANUP_PENDING,
    RSF_D3D11_REDUCED_CLEANUP_REQUIRED, RSF_D3D11_REDUCED_FINISH,
    RSF_D3D11_REDUCED_FRAME_ABI_VERSION, RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED,
    RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED, RSF_D3D11_REDUCED_NONE,
    RSF_D3D11_REDUCED_REQUIRED_RECOVERY_FAILED, RSF_D3D11_REDUCED_RESERVE,
    RSF_D3D11_REDUCED_RESERVED, RSF_D3D11_REDUCED_RETIRE, RSF_D3D11_REDUCED_SEAL,
    RSF_D3D11_REDUCED_SEALED, RSF_D3D11_REDUCED_SPATIAL, RSF_D3D11_REDUCED_SPATIAL_OUTPUT,
    RSF_D3D11_REDUCED_TEMPORAL, RSF_D3D11_REDUCED_TEMPORAL_ATTEMPTED,
    RSF_D3D11_REDUCED_TEMPORAL_OUTPUT, RSF_D3D11_REDUCED_UNSAFE, rsf_d3d11_reduced_frame,
};

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
