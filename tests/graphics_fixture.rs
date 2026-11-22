// SPDX-License-Identifier: MIT
use std::{ffi::c_void, ptr};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

#[path = "graphics_fixture_device.rs"]
mod device;
use device::{Device, TestResult, half_to_f32};
#[path = "colour_transport.rs"]
mod colour_transport;
#[path = "d3d11_state.rs"]
mod d3d11_state;
#[path = "depth_scale.rs"]
mod depth_scale;
#[path = "exposure_normalization.rs"]
mod exposure_normalization;
#[path = "fullscreen_pass.rs"]
mod fullscreen_pass;
#[path = "motion_decode.rs"]
mod motion_decode;
#[path = "motion_resolve.rs"]
mod motion_resolve;
#[path = "native_reinsert.rs"]
mod native_reinsert;
#[path = "resource_roles.rs"]
mod resource_roles;
#[path = "sampler_bias.rs"]
mod sampler_bias;
#[path = "sharpening.rs"]
mod sharpening;
#[path = "ui_layer.rs"]
mod ui_layer;

type Handle = *mut c_void;

fn check(condition: bool, what: &str) -> TestResult {
    if !condition {
        return Err(what.into());
    }
    Ok(())
}

fn near(got: f32, wanted: f32, budget: f32, what: &str) -> TestResult {
    check(
        (got - wanted).abs() <= budget,
        &format!("{what}: got {got}, expected {wanted}"),
    )
}

struct Pass {
    raw: Handle,
    destroy: unsafe extern "C" fn(Handle),
}

impl Drop for Pass {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.raw) };
    }
}

fn run_warp(name: &str, run: impl FnOnce(&Device) -> TestResult) -> i32 {
    let Some(warp) = Device::warp() else {
        return 77;
    };
    match run(&warp) {
        Ok(()) => {
            println!("{name} passed");
            0
        }
        Err(error) => {
            eprintln!("{name} FAILED: {error}");
            1
        }
    }
}
