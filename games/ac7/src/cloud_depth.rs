// SPDX-License-Identifier: MIT
//! TrueSky composite_tile's cloud-depth resource owner and dispatch parameters.
//! The D3D11 leaves read the bound texture descriptors and keep the original shader and dispatch.
use crate::cloud_depth_abi::{
    rsf_ac7_cloud_depth, rsf_ac7_cloud_depth_facts, rsf_ac7_cloud_depth_leaves,
    rsf_ac7_cloud_depth_params,
};
use std::ffi::c_void;
use std::ptr::null_mut;

type Release = unsafe extern "C" fn(*mut c_void);

fn plan(facts: &rsf_ac7_cloud_depth_facts) -> Option<rsf_ac7_cloud_depth_params> {
    if !(16.0..=16_384.0).contains(&facts.viewport_width)
        || !(16.0..=16_384.0).contains(&facts.viewport_height)
        || facts.far_width != facts.grid_width
        || facts.far_height != facts.grid_height
        || facts.grid_width == 0
        || facts.grid_height == 0
        || facts.hdr_bytes < 640
    {
        return None;
    }
    let width = facts.viewport_width as u32;
    let height = facts.viewport_height as u32;
    // The rounded ratio is the measured TrueSky divisor, including one texel of padding.
    let divisor = |pixels: u32, grid: u32| ((pixels + grid / 2) / grid).max(1);
    Some(rsf_ac7_cloud_depth_params {
        size: [width, height],
        divisor: [
            divisor(width, facts.grid_width),
            divisor(height, facts.grid_height),
        ],
        grid: [facts.grid_width, facts.grid_height],
        minimum_km: 0.3,
        minimum_opacity: 0.5,
    })
}

fn release_resources(release: Release, objects: &[*mut c_void]) {
    for &object in objects.iter().filter(|object| !object.is_null()) {
        unsafe { release(object) };
    }
}

fn clear(state: &mut rsf_ac7_cloud_depth, release: Release) {
    release_resources(
        release,
        &[
            state.view,
            state.texture,
            state.constants,
            state.shader,
            state.device,
        ],
    );
    *state = rsf_ac7_cloud_depth::default();
}

/// The native renderer lends state for this call, and each accepted create lends one reference.
#[unsafe(no_mangle)]
extern "C" fn rsf_core_ac7_cloud_depth_prepare(
    state: &mut rsf_ac7_cloud_depth,
    device: *mut c_void,
    facts: &rsf_ac7_cloud_depth_facts,
    out: &mut rsf_ac7_cloud_depth_params,
    leaves: &rsf_ac7_cloud_depth_leaves,
) -> bool {
    let (Some(params), Some(retain), Some(release), Some(create_pipeline), Some(create_target)) = (
        plan(facts),
        leaves.retain,
        leaves.release,
        leaves.create_pipeline,
        leaves.create_target,
    ) else {
        return false;
    };
    if device.is_null() {
        return false;
    }
    if state.device != device {
        clear(state, release);
        unsafe { retain(device) };
        state.device = device;
    }
    if state.refused {
        return false;
    }
    if state.shader.is_null() {
        state.refused = true;
        let mut shader = null_mut();
        let mut constants = null_mut();
        let created = unsafe { create_pipeline(device, &mut shader, &mut constants) };
        if !created || shader.is_null() || constants.is_null() {
            release_resources(release, &[constants, shader]);
            return false;
        }
        state.shader = shader;
        state.constants = constants;
        state.refused = false;
    }
    let [width, height] = params.size;
    if state.texture.is_null() || state.width != width || state.height != height {
        let mut texture = null_mut();
        let mut view = null_mut();
        let created = unsafe { create_target(device, width, height, &mut texture, &mut view) };
        if !created || texture.is_null() || view.is_null() {
            release_resources(release, &[view, texture]);
            return false;
        }
        release_resources(release, &[state.view, state.texture]);
        state.texture = texture;
        state.view = view;
        state.width = width;
        state.height = height;
    }
    *out = params;
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_ac7_cloud_depth_release(
    state: &mut rsf_ac7_cloud_depth,
    leaves: &rsf_ac7_cloud_depth_leaves,
) {
    if let Some(release) = leaves.release {
        clear(state, release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_dispatch_refuses_nonfinite_extent_and_preserves_padded_grid() {
        let mut facts = rsf_ac7_cloud_depth_facts {
            viewport_width: 533.0,
            viewport_height: 300.0,
            far_width: 534,
            far_height: 300,
            grid_width: 534,
            grid_height: 300,
            hdr_bytes: 640,
        };
        let params = plan(&facts).expect("matching cloud bindings");
        assert_eq!(params.size, [533, 300]);
        assert_eq!(params.divisor, [1, 1]);
        assert_eq!(params.grid, [534, 300]);
        facts.viewport_width = f32::NAN;
        assert!(plan(&facts).is_none());
    }
}
