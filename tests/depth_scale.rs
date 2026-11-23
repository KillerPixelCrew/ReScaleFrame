// SPDX-License-Identifier: MIT
use super::*;
use crate::depth_scale::rsf_depth_scale_reserve;
use crate::sys::{
    RSF_DEPTH_SCALE_ABI_VERSION as ABI, rsf_depth_scale_frame as Frame,
    rsf_depth_scale_setup as Setup,
};
use std::ffi::{CStr, c_char};
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_POINTLIST;
use windows::core::Interface;

unsafe extern "C" {
    fn rsf_depth_scale_create(context: Handle, setup: *const Setup, scale: *mut Handle) -> i32;
    fn rsf_depth_scale_prepare(scale: Handle, frame: *const Frame) -> i32;
    fn rsf_depth_scale_commit(scale: Handle) -> i32;
    fn rsf_depth_scale_restore(scale: Handle) -> i32;
    fn rsf_depth_scale_destroy(scale: Handle) -> i32;
}

const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const BUSY: i32 = -4;

unsafe extern "C" fn log(_: Handle, message: *const c_char) {
    eprintln!("{}", unsafe { CStr::from_ptr(message) }.to_string_lossy());
}

unsafe extern "C" fn destroy(scale: Handle) {
    unsafe { rsf_depth_scale_destroy(scale) };
}

fn viewport(context: &ID3D11DeviceContext) -> TestResult<D3D11_VIEWPORT> {
    let mut count = 1;
    let mut viewport = D3D11_VIEWPORT::default();
    unsafe { context.RSGetViewports(&mut count, Some(&mut viewport)) };
    check(count == 1, "wrong viewport count")?;
    Ok(viewport)
}

fn viewport_equal(a: D3D11_VIEWPORT, b: D3D11_VIEWPORT) -> bool {
    [
        a.TopLeftX, a.TopLeftY, a.Width, a.Height, a.MinDepth, a.MaxDepth,
    ]
    .map(f32::to_bits)
        == [
            b.TopLeftX, b.TopLeftY, b.Width, b.Height, b.MinDepth, b.MaxDepth,
        ]
        .map(f32::to_bits)
}

fn buffer(device: &ID3D11Device, desc: &D3D11_BUFFER_DESC) -> TestResult<ID3D11Buffer> {
    let mut result = None;
    unsafe { device.CreateBuffer(desc, None, Some(&mut result)) }?;
    result.ok_or_else(|| "null buffer".into())
}

fn render_identity(
    gpu: &Device,
    pass: Handle,
    frame: &Frame,
    texture: &ID3D11Texture2D,
    desc: &D3D11_TEXTURE2D_DESC,
    original: &[u32],
) -> TestResult {
    let device = &gpu.device;
    let context = &gpu.context;
    let view_desc = D3D11_DEPTH_STENCIL_VIEW_DESC {
        Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
        ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    let mut dsv = None;
    unsafe { device.CreateDepthStencilView(texture, Some(&view_desc), Some(&mut dsv)) }?;
    let dsv = dsv.ok_or("null native DSV")?;
    let append = buffer(
        device,
        &D3D11_BUFFER_DESC {
            ByteWidth: 64,
            BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
            MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
            StructureByteStride: 4,
            ..Default::default()
        },
    )?;
    let uav_desc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
        Format: DXGI_FORMAT_UNKNOWN,
        ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
        Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
            Buffer: D3D11_BUFFER_UAV {
                FirstElement: 0,
                NumElements: 16,
                Flags: D3D11_BUFFER_UAV_FLAG_APPEND.0 as u32,
            },
        },
    };
    let mut uav = None;
    unsafe { device.CreateUnorderedAccessView(&append, Some(&uav_desc), Some(&mut uav)) }?;
    let uav = uav.ok_or("null append UAV")?;
    let count_desc = D3D11_BUFFER_DESC {
        ByteWidth: 4,
        ..Default::default()
    };
    let count = buffer(device, &count_desc)?;
    let count_read = buffer(
        device,
        &D3D11_BUFFER_DESC {
            Usage: D3D11_USAGE_STAGING,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..count_desc
        },
    )?;
    let query = D3D11_QUERY_DESC {
        Query: D3D11_QUERY_OCCLUSION_PREDICATE,
        MiscFlags: 0,
    };
    let mut predicate = None;
    unsafe { device.CreatePredicate(&query, Some(&mut predicate)) }?;
    let predicate = predicate.ok_or("null occlusion predicate")?;
    let counter = 7;
    let vp = D3D11_VIEWPORT {
        TopLeftX: 2.0,
        TopLeftY: 4.0,
        Width: 3.0,
        Height: 5.0,
        MinDepth: 0.2,
        MaxDepth: 0.8,
    };
    unsafe {
        context.Begin(&predicate);
        context.End(&predicate);
        context.SetPredication(&predicate, true);
        context.OMSetRenderTargets(None, &dsv);
        context.RSSetViewports(Some(&[vp]));
        (context.vtable().CSSetUnorderedAccessViews)(
            context.as_raw(),
            4,
            1,
            &uav.as_raw(),
            &counter,
        );
        context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_POINTLIST);
    }
    check(
        unsafe { rsf_depth_scale_prepare(pass, frame) } == 0,
        "identity prepare",
    )?;
    check(
        unsafe { rsf_depth_scale_prepare(pass, frame) } == BUSY,
        "overlapping preparation refused",
    )?;
    check(
        unsafe { gpu.read::<u32>(texture, desc, 1)? } == original,
        "prepare writes scratch only",
    )?;
    check(
        unsafe { rsf_depth_scale_commit(pass) } == 0,
        "identity commit",
    )?;
    check(
        unsafe { gpu.read::<u32>(texture, desc, 1)? } == original,
        "all 256 stencil values and packed depths survive",
    )?;
    check(
        unsafe { rsf_depth_scale_commit(pass) } == BUSY,
        "double commit refused",
    )?;
    let mut current_dsv = None;
    let mut current_uav = [None];
    let mut current_predicate = None;
    let mut predicate_value = Default::default();
    unsafe {
        context.OMGetRenderTargets(None, Some(&mut current_dsv));
        context.CSGetUnorderedAccessViews(4, Some(&mut current_uav));
        context.GetPredication(Some(&mut current_predicate), Some(&mut predicate_value));
    }
    check(
        current_dsv.as_ref().map(Interface::as_raw) == Some(dsv.as_raw())
            && current_uav[0].as_ref().map(Interface::as_raw) == Some(uav.as_raw())
            && current_predicate.as_ref().map(Interface::as_raw) == Some(predicate.as_raw())
            && predicate_value.as_bool(),
        "native depth/UAV/predication bindings restored",
    )?;
    check(
        viewport_equal(vp, viewport(context)?)
            && unsafe { context.IAGetPrimitiveTopology() } == D3D_PRIMITIVE_TOPOLOGY_POINTLIST,
        "viewport and topology restored",
    )?;
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    let actual = unsafe {
        context.SetPredication(None, false);
        context.CopyStructureCount(&count, 0, &uav);
        context.CopyResource(&count_read, &count);
        context.Map(&count_read, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        let actual = mapped.pData.cast::<u32>().as_ref().copied();
        context.Unmap(&count_read, 0);
        context.ClearState();
        actual
    };
    check(actual == Some(counter), "append counter retained")?;
    check(
        unsafe { rsf_depth_scale_restore(pass) } == 0,
        "identity restore",
    )
}

fn sample_axis(i: i32, src: [i32; 2], dst: [i32; 2], jitter: f32) -> usize {
    let uv = (i as f32 + 0.5 - dst[0] as f32) / (dst[1] - dst[0]) as f32;
    let value = (src[0] as f32 + uv * (src[1] - src[0]) as f32 - jitter).floor() as i32;
    value.clamp(src[0], src[1] - 1) as usize
}

fn run(gpu: &Device) -> TestResult {
    let mut original: [u32; 256] =
        std::array::from_fn(|i| (i as u32) << 24 | ((i as u32 * 104729) & 0xff_ffff));
    original[1..5].copy_from_slice(&[0x01ff_ffff, 0x0280_0000, 0x037f_ffff, 0x04ff_fffe]);
    let desc = Device::desc(
        [16, 16],
        DXGI_FORMAT_R24G8_TYPELESS,
        D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE,
    );
    let texture = gpu.initialized(&desc, &original, 64)?;
    let context = gpu.context.as_raw();
    let read = || unsafe { gpu.read::<u32>(&texture, &desc, 1) };
    let mut raw = ptr::null_mut();
    let mut setup = Setup {
        struct_size: size_of::<Setup>() as u32 - 1,
        abi_version: ABI,
        log: Some(log),
        log_user: ptr::null_mut(),
    };
    check(
        unsafe { rsf_depth_scale_create(context, &setup, &mut raw) } == INVALID && raw.is_null(),
        "short setup refused",
    )?;
    setup.struct_size += 1;
    setup.abi_version += 1;
    check(
        unsafe { rsf_depth_scale_create(context, &setup, &mut raw) } == ABI_MISMATCH
            && raw.is_null(),
        "setup ABI refused",
    )?;
    setup.abi_version = ABI;
    let mut deferred = None;
    unsafe { gpu.device.CreateDeferredContext(0, Some(&mut deferred)) }?;
    let deferred = deferred.ok_or("null deferred context")?;
    check(
        unsafe { rsf_depth_scale_create(deferred.as_raw(), &setup, &mut raw) } == INVALID
            && raw.is_null(),
        "deferred context refused",
    )?;
    check(
        unsafe { rsf_depth_scale_create(context, &setup, &mut raw) } == 0 && !raw.is_null(),
        "create depth transaction",
    )?;
    let mut pass = Pass { raw, destroy };

    let vp = D3D11_VIEWPORT {
        TopLeftX: 1.0,
        TopLeftY: 2.0,
        Width: 7.0,
        Height: 9.0,
        MinDepth: 0.2,
        MaxDepth: 0.8,
    };
    unsafe { gpu.context.RSSetViewports(Some(&[vp])) };
    check(
        unsafe { rsf_depth_scale_reserve(raw.cast(), texture.as_raw()) } != 0,
        "allocation-only reserve",
    )?;
    check(
        viewport_equal(vp, viewport(&gpu.context)?) && read()? == original,
        "reserve preserves depth, stencil and viewport",
    )?;
    let unsupported = gpu.texture(
        &D3D11_TEXTURE2D_DESC {
            Format: DXGI_FORMAT_R32_TYPELESS,
            ..desc
        },
        None,
    )?;
    check(
        unsafe { rsf_depth_scale_reserve(raw.cast(), unsupported.as_raw()) } == 0 && read()? == original,
        "unsupported reserve refusal",
    )?;

    let mut frame = Frame {
        struct_size: size_of::<Frame>() as u32,
        abi_version: ABI,
        texture: texture.as_raw(),
        source_rect: [0, 0, 16, 16],
        output_rect: [0, 0, 16, 16],
        jitter_pixels: [0.0; 2],
    };
    check(
        unsafe { rsf_depth_scale_commit(raw) } == INVALID,
        "commit requires prepared image",
    )?;
    frame.struct_size -= 1;
    check(
        unsafe { rsf_depth_scale_prepare(raw, &frame) } == INVALID,
        "short frame refused",
    )?;
    frame.struct_size += 1;
    frame.abi_version += 1;
    check(
        unsafe { rsf_depth_scale_prepare(raw, &frame) } == ABI_MISMATCH,
        "frame ABI refused",
    )?;
    frame.abi_version = ABI;
    frame.source_rect[2] = 17;
    check(
        unsafe { rsf_depth_scale_prepare(raw, &frame) } == INVALID,
        "out-of-bounds source refused",
    )?;
    frame.source_rect[2] = 16;
    frame.jitter_pixels[0] = f32::NAN;
    check(
        unsafe { rsf_depth_scale_prepare(raw, &frame) } == INVALID,
        "nonfinite sampling refused",
    )?;
    frame.jitter_pixels[0] = 0.0;
    check(read()? == original, "refusal leaves texture unchanged")?;
    render_identity(gpu, raw, &frame, &texture, &desc, &original)?;

    let src = [2, 3, 14, 13];
    let dst = [1, 1, 16, 15];
    let jitter = [0.35, -0.6];
    frame.source_rect = src;
    frame.output_rect = dst;
    frame.jitter_pixels = jitter;
    check(
        unsafe { rsf_depth_scale_prepare(raw, &frame) == 0 && rsf_depth_scale_commit(raw) == 0 },
        "scaled depth/stencil commit",
    )?;
    let scaled = read()?;
    check(scaled.len() == original.len(), "size of scaled readback")?;
    for y in 0..16 {
        for x in 0..16 {
            let index = (y * 16 + x) as usize;
            let expected = if (dst[0]..dst[2]).contains(&x) && (dst[1]..dst[3]).contains(&y) {
                let sx = sample_axis(x, [src[0], src[2]], [dst[0], dst[2]], jitter[0]);
                let sy = sample_axis(y, [src[1], src[3]], [dst[1], dst[3]], jitter[1]);
                original[sy * 16 + sx]
            } else {
                original[index]
            };
            check(
                scaled[index] == expected,
                "scaled mapping or outside-pixel preservation",
            )?;
        }
    }
    let pass_addr = raw as usize;
    let texture_addr = texture.as_raw() as usize;
    let refused = std::thread::spawn(move || {
        let raw = pass_addr as Handle;
        let frame = Frame {
            struct_size: size_of::<Frame>() as u32,
            abi_version: ABI,
            texture: texture_addr as Handle,
            source_rect: src,
            output_rect: dst,
            jitter_pixels: jitter,
        };
        unsafe {
            [
                rsf_depth_scale_prepare(raw, &frame),
                rsf_depth_scale_commit(raw),
                rsf_depth_scale_restore(raw),
                rsf_depth_scale_destroy(raw),
            ]
        }
    })
    .join()
    .map_err(|_| "wrong-thread fixture panicked")?;
    check(refused == [BUSY; 4], "foreign thread operations refused")?;
    check(
        unsafe { rsf_depth_scale_restore(raw) } == 0 && read()? == original,
        "scaled restore",
    )?;
    check(
        unsafe { rsf_depth_scale_prepare(raw, &frame) == 0 && rsf_depth_scale_restore(raw) == 0 }
            && read()? == original,
        "uncommitted preparation cancels",
    )?;
    check(
        unsafe { rsf_depth_scale_prepare(raw, &frame) == 0 && rsf_depth_scale_commit(raw) == 0 },
        "prepare committed transaction for destruction",
    )?;
    check(
        unsafe { rsf_depth_scale_destroy(raw) } == 0,
        "destroy restores committed transaction",
    )?;
    pass.raw = ptr::null_mut();
    check(
        read()? == original,
        "destroy restores original depth and stencil",
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_depth_scale_fixture() -> i32 {
    let Some(gpu) = Device::create(D3D_DRIVER_TYPE_WARP, Some(&[D3D_FEATURE_LEVEL_11_0])) else {
        return 77;
    };
    match run(&gpu) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("depth_scale FAILED: {error}");
            1
        }
    }
}
