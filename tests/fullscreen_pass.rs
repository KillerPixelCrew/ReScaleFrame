// SPDX-License-Identifier: MIT
// Verify compositing and state restoration against numeric readback.
use super::*;
use crate::sys::{
    RSF_FULLSCREEN_PASS_ABI_VERSION as ABI, rsf_fullscreen_draw as Draw,
    rsf_fullscreen_setup as Setup,
};
use windows::Win32::Foundation::RECT;
use windows::core::Interface;

unsafe extern "C" {
    fn rsf_fullscreen_pass_create(device: Handle, setup: *const Setup, pass: *mut Handle) -> i32;
    fn rsf_fullscreen_pass_draw(
        pass: Handle,
        context: Handle,
        target: Handle,
        source: Handle,
        draw: *const Draw,
    ) -> i32;
    fn rsf_fullscreen_pass_set_filter(pass: Handle, filter: u32);
    fn rsf_fullscreen_pass_destroy(pass: Handle);
}

const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const SHADER_FAILED: i32 = -3;
const COPY: u32 = 0;
const PREMULTIPLIED: u32 = 2;
const ALPHA: u32 = 3;
const SRGB: u32 = 4;
const GAMMA22: u32 = 5;

fn target(device: &ID3D11Device, texture: &ID3D11Texture2D) -> TestResult<ID3D11RenderTargetView> {
    let mut view = None;
    unsafe { device.CreateRenderTargetView(texture, None, Some(&mut view)) }?;
    view.ok_or_else(|| "null target view".into())
}

fn source(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
) -> TestResult<ID3D11ShaderResourceView> {
    let mut view = None;
    unsafe { device.CreateShaderResourceView(texture, None, Some(&mut view)) }?;
    view.ok_or_else(|| "null source view".into())
}

fn read_pixel(gpu: &Device, texture: &ID3D11Texture2D, x: usize) -> TestResult<[f32; 4]> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    match desc.Format {
        DXGI_FORMAT_R8G8B8A8_UNORM => {
            let pixels = unsafe { gpu.read::<u8>(texture, &desc, 4) }?;
            Ok(std::array::from_fn(|i| {
                f32::from(pixels[x * 4 + i]) / 255.0
            }))
        }
        DXGI_FORMAT_R10G10B10A2_UNORM => {
            let pixel = unsafe { gpu.read::<u32>(texture, &desc, 1) }?[0];
            Ok([
                (pixel & 0x3ff) as f32 / 1023.0,
                ((pixel >> 10) & 0x3ff) as f32 / 1023.0,
                ((pixel >> 20) & 0x3ff) as f32 / 1023.0,
                (pixel >> 30) as f32 / 3.0,
            ])
        }
        _ => Err("unexpected readback format".into()),
    }
}

fn run(gpu: &Device, pass: &Pass) -> TestResult {
    let device = &gpu.device;
    let ctx = &gpu.context;
    let bind = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    let scene_desc = Device::desc([4, 4], DXGI_FORMAT_R10G10B10A2_UNORM, bind);
    let layer_desc = Device::desc([4, 4], DXGI_FORMAT_R8G8B8A8_UNORM, bind);
    let scene = gpu.texture(&scene_desc, None)?;
    let layer = gpu.texture(&layer_desc, None)?;
    let readback = gpu.texture(&layer_desc, None)?;
    let scene_target = target(device, &scene)?;
    let layer_target = target(device, &layer)?;
    let readback_target = target(device, &readback)?;
    let layer_source = source(device, &layer)?;
    let _scene_source = source(device, &scene)?;
    let parameters = Draw {
        struct_size: size_of::<Draw>() as u32,
        mode: COPY,
        exposure: 0.0,
        width: 0,
        height: 0,
    };
    let draw = |target: &ID3D11RenderTargetView, source: &ID3D11ShaderResourceView, mode| {
        let result = unsafe {
            rsf_fullscreen_pass_draw(
                pass.raw,
                ctx.as_raw(),
                target.as_raw(),
                source.as_raw(),
                &Draw { mode, ..parameters },
            )
        };
        check(result == 0, "draw failed")
    };

    let background = [0.4, 0.2, 0.8, 1.0];
    for (ui, expected, budget) in [
        ([0.5; 4], [0.7, 0.6, 0.9], 1.0 / 255.0 + 2.0 / 1023.0),
        ([0.0; 4], [0.4, 0.2, 0.8], 1.0 / 1023.0),
        ([1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 2.0 / 1023.0),
    ] {
        unsafe {
            ctx.ClearRenderTargetView(&layer_target, &ui);
            ctx.ClearRenderTargetView(&scene_target, &background);
        }
        draw(&scene_target, &layer_source, PREMULTIPLIED)?;
        let pixel = read_pixel(gpu, &scene, 0)?;
        for (got, wanted) in pixel.into_iter().zip(expected) {
            near(got, wanted, budget, "premultiplied coverage")?;
        }
    }
    let black = [0.0, 0.0, 0.0, 1.0];
    unsafe { ctx.ClearRenderTargetView(&layer_target, &[0.5, 0.5, 0.5, 1.0]) };
    for (mode, expected) in [(SRGB, 0.7354), (GAMMA22, 0.7297)] {
        unsafe { ctx.ClearRenderTargetView(&scene_target, &black) };
        draw(&scene_target, &layer_source, mode)?;
        near(
            read_pixel(gpu, &scene, 0)?[0],
            expected,
            3.0 / 1023.0,
            "display transfer",
        )?;
    }
    unsafe {
        ctx.ClearRenderTargetView(&scene_target, &black);
        ctx.ClearRenderTargetView(&layer_target, &[0.25, 0.25, 0.25, 0.5]);
    }
    draw(&scene_target, &layer_source, SRGB)?;
    check(
        (0.5..0.65).contains(&read_pixel(gpu, &scene, 0)?[0]),
        "partial sRGB coverage",
    )?;

    unsafe {
        ctx.ClearRenderTargetView(&layer_target, &[0.25, 0.5, 0.75, 0.0]);
        ctx.ClearRenderTargetView(&readback_target, &[1.0; 4]);
    }
    draw(&readback_target, &layer_source, COPY)?;
    for (got, wanted) in read_pixel(gpu, &readback, 0)?
        .into_iter()
        .zip([0.25, 0.5, 0.75, 1.0])
    {
        near(got, wanted, 2.0 / 255.0, "opaque copy")?;
    }

    let ramp_desc = Device::desc(
        [2, 1],
        DXGI_FORMAT_R8G8B8A8_UNORM,
        D3D11_BIND_SHADER_RESOURCE,
    );
    let ramp = gpu.initialized(&ramp_desc, &[0_u8, 0, 0, 255, 255, 0, 0, 255], 8)?;
    let wide_desc = Device::desc([4, 1], DXGI_FORMAT_R8G8B8A8_UNORM, bind);
    let wide = gpu.texture(&wide_desc, None)?;
    let ramp_source = source(device, &ramp)?;
    let wide_target = target(device, &wide)?;
    draw(&wide_target, &ramp_source, COPY)?;
    near(
        read_pixel(gpu, &wide, 1)?[0],
        0.0,
        2.0 / 255.0,
        "default point sampling",
    )?;
    unsafe { rsf_fullscreen_pass_set_filter(pass.raw, 1) };
    draw(&wide_target, &ramp_source, COPY)?;
    near(
        read_pixel(gpu, &wide, 1)?[0],
        0.25,
        3.0 / 255.0,
        "linear sampling at a quarter texel",
    )?;
    unsafe { rsf_fullscreen_pass_set_filter(pass.raw, 0) };

    unsafe { ctx.ClearRenderTargetView(&layer_target, &[1.0, 1.0, 1.0, 0.25]) };
    draw(&readback_target, &layer_source, ALPHA)?;
    for got in read_pixel(gpu, &readback, 0)?.into_iter().take(2) {
        near(got, 0.25, 2.0 / 255.0, "alpha coverage view")?;
    }

    let scissor = RECT {
        left: 1,
        top: 2,
        right: 3,
        bottom: 4,
    };
    let viewport = D3D11_VIEWPORT {
        TopLeftX: 5.0,
        Width: 64.0,
        Height: 32.0,
        MaxDepth: 1.0,
        ..Default::default()
    };
    unsafe {
        ctx.RSSetScissorRects(Some(&[scissor]));
        ctx.RSSetViewports(Some(&[viewport]));
        (ctx.vtable().OMSetRenderTargets)(
            ctx.as_raw(),
            1,
            &readback_target.as_raw(),
            ptr::null_mut(),
        );
    }
    draw(&scene_target, &layer_source, COPY)?;
    let mut count = 1;
    let mut after = RECT::default();
    unsafe { ctx.RSGetScissorRects(&mut count, Some(&mut after)) };
    check(
        count == 1 && [after.left, after.top, after.right, after.bottom] == [1, 2, 3, 4],
        "scissor restored",
    )?;
    let mut after = D3D11_VIEWPORT::default();
    count = 1;
    unsafe { ctx.RSGetViewports(&mut count, Some(&mut after)) };
    check(
        count == 1 && after.Width == 64.0 && after.Height == 32.0 && after.TopLeftX == 5.0,
        "viewport restored",
    )?;
    let mut outputs = [None];
    unsafe { ctx.OMGetRenderTargets(Some(&mut outputs), None) };
    check(
        outputs[0].as_ref().map(Interface::as_raw) == Some(readback_target.as_raw()),
        "render target restored",
    )?;

    for (pass, target, source, params) in [
        (
            ptr::null_mut(),
            scene_target.as_raw(),
            layer_source.as_raw(),
            parameters,
        ),
        (pass.raw, ptr::null_mut(), layer_source.as_raw(), parameters),
        (pass.raw, scene_target.as_raw(), ptr::null_mut(), parameters),
        (
            pass.raw,
            scene_target.as_raw(),
            layer_source.as_raw(),
            Draw {
                mode: 99,
                ..parameters
            },
        ),
        (
            pass.raw,
            scene_target.as_raw(),
            layer_source.as_raw(),
            Draw {
                struct_size: 4,
                ..parameters
            },
        ),
    ] {
        let status =
            unsafe { rsf_fullscreen_pass_draw(pass, ctx.as_raw(), target, source, &params) };
        check(status == INVALID, "invalid fullscreen argument refused")?;
    }
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fullscreen_pass_fixture() -> i32 {
    let Some(gpu) = Device::feature_11() else {
        eprintln!("no D3D11 device available, skipping");
        return 77;
    };
    let mut setup = Setup {
        struct_size: size_of::<Setup>() as u32,
        abi_version: ABI + 1,
        log: None,
        log_user: ptr::null_mut(),
    };
    let mut raw = ptr::null_mut();
    if unsafe { rsf_fullscreen_pass_create(gpu.device.as_raw(), &setup, &mut raw) } != ABI_MISMATCH
    {
        eprintln!("fullscreen pass accepted an invalid ABI");
        return 1;
    }
    setup.abi_version = ABI;
    match unsafe { rsf_fullscreen_pass_create(gpu.device.as_raw(), &setup, &mut raw) } {
        SHADER_FAILED => return 77,
        0 if !raw.is_null() => {}
        _ => return 1,
    }
    let pass = Pass {
        raw,
        destroy: rsf_fullscreen_pass_destroy,
    };
    let result = run(&gpu, &pass);
    drop(pass);
    unsafe { rsf_fullscreen_pass_destroy(ptr::null_mut()) };
    match result {
        Ok(()) => {
            eprintln!("fullscreen_pass: all checks passed");
            0
        }
        Err(error) => {
            eprintln!("fullscreen_pass FAILED: {error}");
            1
        }
    }
}
