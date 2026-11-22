// SPDX-License-Identifier: MIT
#![cfg(all(windows, not(test)))]

#[allow(dead_code)]
#[path = "../../graphics_fixture_device.rs"]
mod device;
#[path = "../../../runtime/orchestrator/src/native_translucency_abi.rs"]
mod native_translucency_abi;

use device::{Device, TestResult, half_to_f32};
use native_translucency_abi::rsf_native_translucency_masks as Masks;
use rsf_game_sys::{
    RSF_GAME_RENDER_CLOUD_DEPTH, RSF_GAME_RENDER_MATERIALS, RSF_GAME_RENDER_PRIMARY,
    RSF_GAME_RENDER_SR, RSF_GAME_RENDER_TRANSLUCENCY, RSF_GAME_RENDER_TRANSLUCENCY_LAYER,
    rsf_game_render_pass as Pass,
};
use std::ffi::c_void;
use std::ptr::{self, null, null_mut};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

type Handle = *mut c_void;

unsafe extern "C" {
    fn rsf_native_translucency_pass(context: Handle, pass: *const Pass, begin: u32);
    fn rsf_native_translucency_take(pass: *const Pass, masks: *mut Masks) -> i32;
    fn rsf_native_translucency_release();
}

const _: () = {
    assert!(size_of::<Masks>() == 56);
    assert!(std::mem::offset_of!(Masks, motion_depth) == 48);
};

fn check(condition: bool, reason: &str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(reason.into())
    }
}

fn render_pass(role: u32, frame: u64, flags: u32, rect: [i32; 4]) -> Pass {
    // The SDK record contains only integers, floats, and nullable pointers.
    let mut pass: Pass = unsafe { std::mem::zeroed() };
    pass.struct_size = size_of::<Pass>() as u32;
    pass.role = role;
    pass.native_frame = frame;
    pass.family_key = 1;
    pass.view_key = 2;
    pass.flags = RSF_GAME_RENDER_PRIMARY | flags;
    pass.render_rect = rect;
    pass
}

fn masks() -> Masks {
    Masks {
        struct_size: size_of::<Masks>() as u32,
        ..Default::default()
    }
}

fn empty(masks: &Masks) -> bool {
    [
        masks.reactive,
        masks.coverage,
        masks.bias,
        masks.color_before_transparency,
        masks.transparency_layer,
        masks.motion_depth,
    ]
    .into_iter()
    .all(|item| item.is_null())
}

fn abi_contract() -> TestResult {
    let rect = [0, 0, 8, 4];
    let sr = render_pass(RSF_GAME_RENDER_SR, 3, 0, rect);
    let mut out = masks();
    check(
        unsafe { rsf_native_translucency_take(null(), &mut out) } == 0,
        "null SR pass refused",
    )?;
    check(
        unsafe { rsf_native_translucency_take(&sr, null_mut()) } == 0,
        "null output refused",
    )?;
    out.struct_size -= 1;
    check(
        unsafe { rsf_native_translucency_take(&sr, &mut out) } == 0,
        "short output refused",
    )?;
    out.struct_size = size_of::<Masks>() as u32;
    let mut short = render_pass(RSF_GAME_RENDER_SR, 3, 0, rect);
    short.struct_size -= 1;
    check(
        unsafe { rsf_native_translucency_take(&short, &mut out) } == 0,
        "short SR pass refused",
    )?;
    check(
        unsafe { rsf_native_translucency_take(&sr, &mut out) } == 0,
        "no output before scope",
    )?;
    check(empty(&out), "refusal preserves output pointers")?;
    for role in [
        RSF_GAME_RENDER_TRANSLUCENCY,
        RSF_GAME_RENDER_CLOUD_DEPTH,
        RSF_GAME_RENDER_MATERIALS,
    ] {
        let pass = render_pass(role, 3, 0, rect);
        unsafe {
            rsf_native_translucency_pass(null_mut(), &pass, 1);
            rsf_native_translucency_pass(null_mut(), &pass, 0);
        }
    }
    unsafe { rsf_native_translucency_pass(null_mut(), null(), 1) };
    check(
        unsafe { rsf_native_translucency_take(&sr, &mut out) } == 0,
        "ignored passes publish nothing",
    )?;
    unsafe {
        rsf_native_translucency_release();
        rsf_native_translucency_release();
    }
    check(
        unsafe { rsf_native_translucency_take(&sr, &mut out) } == 0,
        "no output after release",
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_translucency_abi_fixture() -> i32 {
    report("Native translucency ABI", abi_contract())
}

const RECT: [i32; 4] = [4, 2, 12, 6];
// IEEE binary16 values for the exact fixtures, including the rounded representation of 0.2.
const ZERO: u16 = 0;
const ONE: u16 = 0x3c00;
const HALF: u16 = 0x3800;
const FOUR: u16 = 0x4400;
const ONE_FIFTH: u16 = 0x3266;

fn rgba(gpu: &Device, rgb: u16, alpha: u16) -> TestResult<ID3D11Texture2D> {
    let texels = [[rgb, rgb, rgb, alpha]; 16 * 8];
    gpu.initialized(
        &Device::desc(
            [16, 8],
            DXGI_FORMAT_R16G16B16A16_FLOAT,
            D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET,
        ),
        &texels,
        16 * 8,
    )
}

fn update<T>(gpu: &Device, texture: &ID3D11Texture2D, [x, y]: [u32; 2], texel: &T) {
    let bounds = D3D11_BOX {
        left: x,
        top: y,
        front: 0,
        right: x + 1,
        bottom: y + 1,
        back: 1,
    };
    let stride = size_of::<T>() as u32;
    unsafe {
        gpu.context.UpdateSubresource(
            texture,
            0,
            Some(&bounds),
            ptr::from_ref(texel).cast(),
            stride,
            stride,
        )
    };
}

fn read(gpu: &Device, texture: Handle) -> TestResult<Vec<f32>> {
    let texture =
        unsafe { ID3D11Texture2D::from_raw_borrowed(&texture) }.ok_or("missing mask texture")?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    check(
        [desc.Width, desc.Height] == [8, 4],
        "mask resource is rect-local",
    )?;
    match desc.Format {
        DXGI_FORMAT_R32_FLOAT => unsafe { gpu.read::<f32>(texture, &desc, 1) },
        DXGI_FORMAT_R16G16B16A16_FLOAT => {
            let pixels = unsafe { gpu.read::<u16>(texture, &desc, 4) }?;
            Ok(pixels
                .chunks_exact(4)
                .map(|pixel| half_to_f32(pixel[0]))
                .collect())
        }
        _ => Err("unexpected mask readback format".into()),
    }
}

fn scope(frame: u64, scene: &ID3D11Texture2D) -> Pass {
    let mut pass = render_pass(RSF_GAME_RENDER_TRANSLUCENCY, frame, 0, RECT);
    pass.color_input = scene.as_raw();
    pass
}

fn take(frame: u64) -> TestResult<Masks> {
    let sr = render_pass(RSF_GAME_RENDER_SR, frame, 0, RECT);
    let mut out = masks();
    check(
        unsafe { rsf_native_translucency_take(&sr, &mut out) } == 1,
        "matching SR frame has masks",
    )?;
    Ok(out)
}

struct Retire;
impl Drop for Retire {
    fn drop(&mut self) {
        unsafe { rsf_native_translucency_release() };
    }
}

fn device_contract(gpu: &Device) -> TestResult {
    let _retire = Retire;
    let context = gpu.context.as_raw();
    let report_pass =
        |pass: &Pass, begin| unsafe { rsf_native_translucency_pass(context, pass, begin) };
    let (scene, layer) = (rgba(gpu, HALF, ONE)?, rgba(gpu, ZERO, ONE)?);
    let standard = scope(3, &scene);
    report_pass(&standard, 1);
    update(gpu, &scene, [5, 3], &[FOUR, FOUR, FOUR, ONE]);
    update(gpu, &scene, [0, 0], &[FOUR, FOUR, FOUR, ONE]);
    report_pass(&standard, 0);

    let mut foreign = render_pass(RSF_GAME_RENDER_SR, 3, 0, RECT);
    foreign.session_id = 9;
    let mut refused = masks();
    check(
        unsafe { rsf_native_translucency_take(&foreign, &mut refused) } == 0
            && refused.reactive.is_null(),
        "a foreign session cannot take or close the mask scope",
    )?;
    let mut separate = scope(3, &scene);
    separate.flags |= RSF_GAME_RENDER_TRANSLUCENCY_LAYER;
    report_pass(&separate, 1);
    update(gpu, &layer, [8, 4], &[ZERO, ZERO, ZERO, ONE_FIFTH]);
    separate.color_output = layer.as_raw();
    report_pass(&separate, 0);

    let mut out = take(3)?;
    check(
        out.motion_depth.is_null(),
        "no cloud depth before any composite",
    )?;
    let reactive = read(gpu, out.reactive)?;
    let coverage = read(gpu, out.coverage)?;
    let bias = read(gpu, out.bias)?;
    let opaque = read(gpu, out.color_before_transparency)?;
    let _copied_layer = read(gpu, out.transparency_layer)?;
    let (drawn, smoke, untouched) = (9, 20, 0);
    check(
        (reactive[drawn] - (0.8 - 1.0 / 3.0)).abs() < 0.01
            && coverage[drawn] == 1.0
            && bias[drawn] == 1.0,
        "bright draw has tonemapped reactivity and full coverage",
    )?;
    check(
        (reactive[smoke] - 0.8).abs() < 0.01 && coverage[smoke] == 1.0 && bias[smoke] == 1.0,
        "layer transmittance contributes to masks",
    )?;
    check(
        reactive[untouched] == 0.0 && coverage[untouched] == 0.0 && bias[untouched] == 0.0,
        "untouched pixels and outside-rect writes do not contribute",
    )?;
    check(opaque[drawn] == 0.5, "opaque snapshot precedes the draw")?;
    let stale = render_pass(RSF_GAME_RENDER_SR, 4, 0, RECT);
    check(
        unsafe { rsf_native_translucency_take(&stale, &mut out) } == 0,
        "different native frame refused",
    )?;

    let mut depth = [0.0f32; 32];
    depth[9] = 0.25;
    let clouds = gpu.initialized(
        &Device::desc([8, 4], DXGI_FORMAT_R32_FLOAT, D3D11_BIND_SHADER_RESOURCE),
        &depth,
        32,
    )?;
    let mut cloud = render_pass(RSF_GAME_RENDER_CLOUD_DEPTH, 0, 0, RECT);
    cloud.depth = clouds.as_raw();
    let frame = |number| {
        let pass = scope(number, &scene);
        report_pass(&pass, 1);
        report_pass(&pass, 0);
        take(number)
    };
    report_pass(&cloud, 1);
    let adopted = frame(4)?;
    check(
        adopted.transparency_layer.is_null() && read(gpu, adopted.motion_depth)?[9] == 0.25,
        "prepass cloud depth adopted without a stale layer",
    )?;
    check(
        frame(5)?.motion_depth.is_null(),
        "next frame requires a fresh cloud composite",
    )?;

    let mut foreign_cloud = render_pass(RSF_GAME_RENDER_CLOUD_DEPTH, 0, 0, RECT);
    foreign_cloud.depth = clouds.as_raw();
    foreign_cloud.session_id = 9;
    report_pass(&foreign_cloud, 1);
    check(
        frame(6)?.motion_depth.is_null(),
        "foreign-session cloud depth refused",
    )?;
    let late = scope(7, &scene);
    report_pass(&late, 1);
    report_pass(&cloud, 1);
    report_pass(&late, 0);
    check(
        take(7)?.motion_depth.is_null(),
        "late composite misses its frame",
    )?;
    check(
        frame(8)?.motion_depth.is_null(),
        "late composite does not leak into the next frame",
    )?;

    report_pass(&cloud, 1);
    let borrowed = frame(9)?.motion_depth;
    check(!borrowed.is_null(), "first cloud depth was published")?;
    update(gpu, &clouds, [1, 1], &0.5f32);
    report_pass(&cloud, 1);
    check(
        read(gpu, borrowed)?[9] == 0.25,
        "borrowed depth survives the next composite",
    )?;
    let latest = frame(10)?.motion_depth;
    check(
        !latest.is_null() && latest != borrowed,
        "next scope adopts the other cloud slot",
    )?;
    check(
        read(gpu, latest)?[9] == 0.5,
        "new cloud depth contains the later write",
    )
}

fn report(name: &str, result: TestResult) -> i32 {
    if let Err(error) = result {
        eprintln!("{name}: {error}");
        1
    } else {
        println!("{name} passed");
        0
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_translucency_fixture() -> i32 {
    let Some(gpu) = Device::warp() else {
        return 77;
    };
    report("Native translucency", device_contract(&gpu))
}
