// SPDX-License-Identifier: MIT
// Verify stored Unreal-biased examples, pre-decode sentinel checks and optional validity.
use super::*;
use crate::graphics_pass_abi::{
    RSF_MOTION_DECODE_ABI_VERSION as ABI, rsf_motion_decode_setup as Setup,
};
use rsf_game_sys::sr_abi::rsf_motion_decode_params as Params;
use std::ffi::{CStr, c_char};
use windows::core::Interface;

unsafe extern "C" {
    fn rsf_motion_decode_create(device: Handle, setup: *const Setup, pass: *mut Handle) -> i32;
    fn rsf_motion_decode_run(
        pass: Handle,
        context: Handle,
        source: Handle,
        params: *const Params,
    ) -> i32;
    fn rsf_motion_decode_texture(pass: Handle) -> Handle;
    fn rsf_motion_decode_destroy(pass: Handle);
}

const SCALE: f32 = 0.499 * 0.5;
const BIAS: f32 = 32767.0 / 65535.0;
const SENTINEL: f32 = -1000.0;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const SHADER_FAILED: i32 = -3;
const SOURCE_MISMATCH: i32 = -5;

unsafe extern "C" fn collect(_: Handle, message: *const c_char) {
    let text = unsafe { CStr::from_ptr(message) }.to_string_lossy();
    eprintln!("[decode] {text}");
}

fn encode(velocity: f32) -> u16 {
    ((velocity * SCALE + BIAS) * 65535.0 + 0.5) as u16
}

fn run(gpu: &Device) -> TestResult {
    let size = [4, 1];
    let wanted = [[0.0, 0.0], [0.0, 0.0], [0.5, 0.25], [-0.5, -1.0]];
    let mut pixels = wanted.map(|pixel| pixel.map(encode));
    pixels[0] = [0; 2];
    let desc = Device::desc(size, DXGI_FORMAT_R16G16_UNORM, D3D11_BIND_SHADER_RESOURCE);
    let motion = gpu.initialized(&desc, &pixels, 16)?;

    let mut setup = Setup {
        struct_size: size_of::<Setup>() as u32,
        abi_version: ABI + 1,
        width: size[0],
        height: size[1],
        output_format: 0,
        log: Some(collect),
        log_user: ptr::null_mut(),
    };
    let mut handle = ptr::null_mut();
    let dev = gpu.device.as_raw();
    check(
        unsafe { rsf_motion_decode_create(dev, &setup, &mut handle) } == ABI_MISMATCH,
        "setup ABI",
    )?;
    setup.abi_version = ABI;
    setup.width = 0;
    check(
        unsafe { rsf_motion_decode_create(dev, &setup, &mut handle) } == INVALID_ARGUMENT,
        "zero width",
    )?;
    setup.width = size[0];
    let status = unsafe { rsf_motion_decode_create(dev, &setup, &mut handle) };
    if status == SHADER_FAILED {
        eprintln!("no shader compiler available, skipping the decode");
        return Ok(());
    }
    check(status == 0 && !handle.is_null(), "create decode pass")?;
    let pass = Pass {
        raw: handle,
        destroy: rsf_motion_decode_destroy,
    };
    let mut params = Params {
        struct_size: size_of::<Params>() as u32,
        scale_x: 1.0 / SCALE,
        scale_y: 1.0 / SCALE,
        bias_x: BIAS,
        bias_y: BIAS,
        output_scale_x: 1.0,
        output_scale_y: 1.0,
        invalid_value: SENTINEL,
        zero_means_unwritten: 1,
        validity_enabled: 0,
        validity_channel: 0,
        validity_threshold: 0.0,
    };
    let dispatch = |source: &ID3D11Texture2D, params: &Params| unsafe {
        rsf_motion_decode_run(pass.raw, gpu.context.as_raw(), source.as_raw(), params)
    };
    let wider = D3D11_TEXTURE2D_DESC { Width: 8, ..desc };
    let mismatch = gpu.initialized(&wider, &[0_u16; 16], 32)?;
    check(
        dispatch(&mismatch, &params) == SOURCE_MISMATCH,
        "wrong source extent",
    )?;
    check(dispatch(&motion, &params) == 0, "decode run")?;
    let decoded = unsafe { rsf_motion_decode_texture(pass.raw) };
    let decoded =
        unsafe { ID3D11Texture2D::from_raw_borrowed(&decoded) }.ok_or("missing output motion")?;
    let mut output = D3D11_TEXTURE2D_DESC::default();
    unsafe { decoded.GetDesc(&mut output) };
    check(
        output.Format == DXGI_FORMAT_R16G16_FLOAT,
        "default output motion format",
    )?;
    let read = || -> TestResult<Vec<f32>> {
        Ok(unsafe { gpu.read::<u16>(decoded, &output, 2) }?
            .into_iter()
            .map(half_to_f32)
            .collect())
    };
    let values = read()?;
    for (index, &value) in values.iter().take(8).enumerate() {
        if index < 2 {
            near(
                value,
                SENTINEL,
                1.0,
                "clear value decodes to both sentinel channels",
            )?;
        } else {
            near(value, wanted[index / 2][index % 2], 0.01, "decoded motion")?;
        }
    }
    params.output_scale_y = -1.0;
    check(dispatch(&motion, &params) == 0, "flip output Y")?;
    let values = read()?;
    near(values[4], wanted[2][0], 0.01, "output X")?;
    near(values[5], -wanted[2][1], 0.01, "flipped output Y")?;

    let validity_desc = D3D11_TEXTURE2D_DESC {
        Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
        ..desc
    };
    let validity_samples: [[u16; 4]; 4] = [
        [0, 0, 0x3c00, 0],
        [0, 0, 0, 0],
        [0x3800, 0xb400, 0x3c00, 0],
        [0x3800, 0xb400, 0, 0],
    ];
    let validity = gpu.initialized(&validity_desc, &validity_samples, 32)?;
    let mut vp = Params {
        scale_x: 1.0,
        scale_y: 1.0,
        bias_x: 0.0,
        bias_y: 0.0,
        output_scale_x: 1.0,
        output_scale_y: 1.0,
        validity_enabled: 1,
        validity_channel: 2,
        validity_threshold: 0.5,
        ..params
    };
    check(dispatch(&validity, &vp) == 0, "run with a validity channel")?;
    let values = read()?;
    for (index, expected, budget) in [
        (0, 0.0, 0.001),
        (1, 0.0, 0.001),
        (2, SENTINEL, 1.0),
        (3, SENTINEL, 1.0),
        (4, 0.5, 0.001),
        (5, -0.25, 0.001),
        (6, SENTINEL, 1.0),
    ] {
        near(
            values[index],
            expected,
            budget,
            "validity overrides stored zero and written motion",
        )?;
    }
    check(
        dispatch(
            &validity,
            &Params {
                validity_enabled: 0,
                ..vp
            },
        ) == 0,
        "validity off",
    )?;
    let values = read()?;
    for (index, expected, budget) in [
        (0, SENTINEL, 1.0),
        (2, SENTINEL, 1.0),
        (4, 0.5, 0.001),
        (6, 0.5, 0.001),
    ] {
        near(
            values[index],
            expected,
            budget,
            "stored zero test with validity disabled",
        )?;
    }
    let legacy = Params {
        struct_size: std::mem::offset_of!(Params, validity_enabled) as u32,
        ..vp
    };
    check(
        dispatch(&validity, &legacy) == 0,
        "pre-validity structure size",
    )?;
    let values = read()?;
    near(values[0], SENTINEL, 1.0, "legacy stored zero test")?;
    near(values[6], 0.5, 0.001, "legacy structure ignores validity")?;
    vp.validity_channel = 4;
    check(
        dispatch(&validity, &vp) == INVALID_ARGUMENT,
        "out-of-range validity channel",
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_motion_decode_fixture() -> i32 {
    let Some(gpu) = Device::feature_11() else {
        eprintln!("no D3D11 device available, skipping");
        return 0;
    };
    match run(&gpu) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("motion_decode FAILED: {error}");
            1
        }
    }
}
