// SPDX-License-Identifier: MIT
use super::*;
use crate::render_abi::rsf_motion_resolve_params as Params;
use windows::core::Interface;

unsafe extern "C" {
    fn rsf_motion_resolve_create(device: Handle, width: u32, height: u32, pass: *mut Handle)
    -> i32;
    fn rsf_motion_resolve_run(
        pass: Handle,
        context: Handle,
        motion: Handle,
        depth: Handle,
        params: *const Params,
    ) -> i32;
    fn rsf_motion_resolve_motion(pass: Handle) -> Handle;
    fn rsf_motion_resolve_depth(pass: Handle) -> Handle;
    fn rsf_motion_resolve_destroy(pass: Handle);
}

fn read_motion(warp: &Device, pass: &Pass) -> TestResult<Vec<f32>> {
    let raw = unsafe { rsf_motion_resolve_motion(pass.raw) };
    let output =
        unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.ok_or("missing motion output")?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { output.GetDesc(&mut desc) };
    if desc.Format != DXGI_FORMAT_R16G16_FLOAT {
        return Err("wrong output motion format".into());
    }
    Ok(unsafe { warp.read::<u16>(output, &desc, 2) }?
        .into_iter()
        .map(half_to_f32)
        .collect())
}

fn read_depth(warp: &Device, pass: &Pass) -> TestResult<Vec<f32>> {
    let raw = unsafe { rsf_motion_resolve_depth(pass.raw) };
    let output =
        unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.ok_or("missing depth output")?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { output.GetDesc(&mut desc) };
    if desc.Format != DXGI_FORMAT_R32_FLOAT {
        return Err("wrong output depth format".into());
    }
    unsafe { warp.read(output, &desc, 1) }
}

fn run(warp: &Device) -> TestResult {
    let size = [8, 8];
    let mut pixels = [[-1000.0_f32; 2]; 64];
    pixels[0] = [0.0; 2];
    pixels[1] = [0.5, -0.5];
    let motion_desc = Device::desc(size, DXGI_FORMAT_R32G32_FLOAT, D3D11_BIND_SHADER_RESOURCE);
    let depth_desc = Device::desc(size, DXGI_FORMAT_R32_FLOAT, D3D11_BIND_SHADER_RESOURCE);
    let motion = warp.initialized(&motion_desc, &pixels, 64)?;
    let depth = warp.initialized(&depth_desc, &[0.5_f32; 64], 32)?;
    let mut raw = ptr::null_mut();
    if unsafe { rsf_motion_resolve_create(warp.device.as_raw(), 8, 8, &mut raw) } == 0 {
        return Err("create failed".into());
    }
    let pass = Pass {
        raw,
        destroy: rsf_motion_resolve_destroy,
    };
    let resolve = |depth: &ID3D11Texture2D, params: &Params| unsafe {
        rsf_motion_resolve_run(
            pass.raw,
            warp.context.as_raw(),
            motion.as_raw(),
            depth.as_raw(),
            params,
        ) != 0
    };
    let mut params = Params {
        struct_size: size_of::<Params>() as u32,
        clip_to_previous: [0.0; 16],
        decoded_to_pixels: [4.0, -4.0],
        sentinel: -1000.0,
        has_sentinel: 1,
        depth_layer: ptr::null_mut(),
        jitter_pixels: [0.0; 2],
    };
    for index in [0, 5, 10, 15] {
        params.clip_to_previous[index] = 1.0;
    }
    params.clip_to_previous[12] = 0.25;
    let mut passed = resolve(&depth, &params);
    let fields = read_motion(warp, &pass)?;
    eprintln!("motion: {:?}", &fields[..6]);
    // Valid zero, written motion, then camera reprojection on the sentinel.
    passed &= fields[..6] == [0.0, 0.0, 2.0, 2.0, 1.0, 0.0];
    passed &= read_depth(warp, &pass)?[0] == 0.5;

    // Canonical UV motion with no sentinel and native D24 depth.
    let native_desc = Device::desc(
        size,
        DXGI_FORMAT_R24G8_TYPELESS,
        D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_DEPTH_STENCIL,
    );
    let native_depth = warp.initialized(&native_desc, &[0x0080_0000_u32; 64], 32)?;
    let normalized = Params {
        has_sentinel: 0,
        sentinel: 0.0,
        decoded_to_pixels: [-8.0; 2],
        ..params
    };
    passed &= resolve(&native_depth, &normalized);
    passed &= read_motion(warp, &pass)?[..5] == [0.0, 0.0, -4.0, 4.0, 8000.0];
    passed &= (read_depth(warp, &pass)?[0] - 0.5).abs() < 1e-6;

    let mut layer = [0.0_f32; 64];
    layer[0] = 0.5;
    layer[2] = 0.5;
    let sky = warp.initialized(&depth_desc, &[0.0_f32; 64], 32)?;
    let depth_layer = warp.initialized(&depth_desc, &layer, 32)?;
    let mut layered = params;
    layered.clip_to_previous[12] = 0.0;
    layered.clip_to_previous[8] = 0.5;
    layered.depth_layer = depth_layer.as_raw();
    passed &= resolve(&sky, &layered);
    let fields = read_motion(warp, &pass)?;
    eprintln!("layered: {:?}", &fields[..8]);
    passed &=
        fields[0..2] == [0.0; 2] && (fields[4] - 1.0).abs() < 1e-5 && fields[5..8] == [0.0; 3];

    // An omitted layer is valid. Supplied layers with a bad format, size, device or SRV must fail.
    layered.depth_layer = ptr::null_mut();
    passed &= resolve(&sky, &layered);
    layered.depth_layer = motion.as_raw();
    passed &= !resolve(&sky, &layered);
    let unreadable = warp.texture(
        &D3D11_TEXTURE2D_DESC {
            BindFlags: 0,
            ..depth_desc
        },
        None,
    )?;
    layered.depth_layer = unreadable.as_raw();
    passed &= !resolve(&sky, &layered);
    let small = warp.texture(
        &D3D11_TEXTURE2D_DESC {
            Width: 4,
            ..depth_desc
        },
        None,
    )?;
    layered.depth_layer = small.as_raw();
    passed &= !resolve(&sky, &layered);
    let other = Device::warp().ok_or("second WARP device unavailable")?;
    let foreign = other.texture(&depth_desc, None)?;
    layered.depth_layer = foreign.as_raw();
    passed &= !resolve(&sky, &layered);

    params.clip_to_previous[0] = f32::NAN;
    passed &= !resolve(&depth, &params);
    if !passed {
        return Err("motion, depth, layer or invalid-parameter assertion failed".into());
    }
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_motion_resolve_fixture() -> i32 {
    run_warp("motion_resolve", run)
}
