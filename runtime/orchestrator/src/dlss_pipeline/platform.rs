// SPDX-License-Identifier: MIT
// The production leaves of the D3D11 SR pipeline; dlss_pipeline_tests.rs defines fakes with the
// same names and signatures.
use super::*;
use crate::colour_d3d11::{
    rsf_colour_fidelity_create, rsf_colour_fidelity_destroy, rsf_colour_fidelity_run,
    rsf_colour_transport_create, rsf_colour_transport_decode, rsf_colour_transport_destroy,
    rsf_colour_transport_encode,
};
use crate::d3d11_d3d12::borrow;
pub(super) use crate::d3d11_output_platform::{
    rsf_d3d11_output_create_texture as create_texture,
    rsf_d3d11_output_texture_desc as texture_desc,
};
pub(super) use crate::d3d11_present::rsf_d3d11_present_host as present_host;
pub(super) use crate::d3d11_sharpening::rsf_core_sharpen11_destroy as sharpen_destroy;
use crate::d3d11_sharpening::rsf_core_sharpen11_run;
pub(super) use crate::dlss_streamline::exports::{
    rsf_dlss_evaluate as evaluate_layer, rsf_dlss_load as load,
    rsf_dlss_query_support as query_support, rsf_dlss_release_resources as release_resources,
    rsf_dlss_set_device as set_device, rsf_dlss_share_host as share_host,
    rsf_dlss_shutdown as shutdown,
};
pub(super) use crate::dlss_streamline::{
    rsf_dlss_plan_render_size as plan, rsf_dlss_release_viewport as release_viewport,
};
pub(super) use crate::input_dump::rsf_input_dump_texture as input_dump;
use crate::input_dump::{Log, TextureOptions};
use crate::motion_decode::{
    rsf_motion_decode_create, rsf_motion_decode_destroy, rsf_motion_decode_run,
    rsf_motion_decode_texture,
};
use crate::motion_resolve::{
    rsf_motion_resolve_create, rsf_motion_resolve_depth, rsf_motion_resolve_destroy,
    rsf_motion_resolve_motion, rsf_motion_resolve_run,
};
pub(super) use crate::native_fg::{
    rsf_native_fg_capture as fg_capture, rsf_native_fg_evaluate as fg_evaluate,
};
pub(super) use crate::native_sr::rsf_native_sr_release_resources as native_sr_release;
use crate::sr_legacy::rsf_sr_legacy_create;
use crate::sys::rsf_motion_resolve_params;
use crate::sys::{RSF_MOTION_DECODE_ABI_VERSION, rsf_motion_decode_setup as DecodeSetup};
use rsf_graphics::texture_dump::{rsf_dump_texture, rsf_dump_texture_bytes};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

unsafe extern "C" {
    fn rsf_sharpening_requested(options: &Sharpening) -> u32;
}

pub(super) unsafe fn clear_black(device: Raw, texture: Raw) {
    let device = unsafe { borrow::<ID3D11Device>(&device) };
    let texture = unsafe { borrow::<ID3D11Texture2D>(&texture) };
    let Ok(context) = (unsafe { device.GetImmediateContext() }) else {
        return;
    };
    let mut view = None;
    if unsafe { device.CreateRenderTargetView(texture, None, Some(&mut view)) }.is_ok()
        && let Some(view) = view
    {
        unsafe { context.ClearRenderTargetView(&view, &[0.0, 0.0, 0.0, 1.0]) };
    }
}

pub(super) unsafe fn create_zeroed(device: Raw, desc: *const TextureDesc, row_pitch: u32) -> Raw {
    let desc = unsafe { &*desc };
    let native = D3D11_TEXTURE2D_DESC {
        Width: desc.width,
        Height: desc.height,
        MipLevels: desc.mips,
        ArraySize: desc.layers,
        Format: DXGI_FORMAT(desc.format as i32),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: desc.samples,
            Quality: desc.sample_quality,
        },
        Usage: D3D11_USAGE(desc.usage as i32),
        BindFlags: desc.bind_flags,
        CPUAccessFlags: desc.cpu_access_flags,
        MiscFlags: desc.misc_flags,
    };
    let length = row_pitch as usize * desc.height as usize;
    let mut zeros = Vec::<u8>::new();
    if zeros.try_reserve_exact(length).is_err() {
        return null_mut();
    }
    zeros.resize(length, 0);
    let initial = D3D11_SUBRESOURCE_DATA {
        pSysMem: zeros.as_ptr().cast(),
        SysMemPitch: row_pitch,
        SysMemSlicePitch: 0,
    };
    let mut result = None;
    let device = unsafe { borrow::<ID3D11Device>(&device) };
    if unsafe {
        device.CreateTexture2D(
            &native,
            Some(std::ptr::from_ref(&initial)),
            Some(&mut result),
        )
    }
    .is_err()
    {
        return null_mut();
    }
    result.map_or(null_mut(), Interface::into_raw)
}

pub(super) unsafe fn decode_create(
    device: Raw,
    width: u32,
    height: u32,
    format: u32,
    log: rsf_dlss_pipeline_log_fn,
    out: *mut Raw,
) -> i32 {
    let setup = DecodeSetup {
        struct_size: size_of::<DecodeSetup>() as u32,
        abi_version: RSF_MOTION_DECODE_ABI_VERSION,
        width,
        height,
        output_format: format,
        log,
        log_user: null_mut(),
    };
    unsafe { rsf_motion_decode_create(device, &setup, out.cast()) }
}

// The pass owners type their pass pointers; the pipeline keeps them as raw handles.
pub(super) unsafe fn decode_run(
    pass: Raw,
    context: Raw,
    motion: Raw,
    params: *const DecodeParams,
) -> i32 {
    unsafe { rsf_motion_decode_run(pass.cast(), context, motion, params) }
}

pub(super) unsafe fn decode_texture(pass: Raw) -> Raw {
    unsafe { rsf_motion_decode_texture(pass.cast()) }
}

pub(super) unsafe fn decode_destroy(pass: Raw) {
    unsafe { rsf_motion_decode_destroy(pass.cast()) };
}

pub(super) unsafe fn resolve_create(device: Raw, width: u32, height: u32) -> Raw {
    let mut pass = null_mut();
    if unsafe { rsf_motion_resolve_create(device, width, height, &mut pass) } == 0 {
        return null_mut();
    }
    pass.cast()
}

pub(super) unsafe fn resolve_run(
    pass: Raw,
    context: Raw,
    motion: Raw,
    depth: Raw,
    params: *const rsf_motion_resolve_params,
) -> i32 {
    unsafe { rsf_motion_resolve_run(pass.cast(), context, motion, depth, params) }
}

pub(super) unsafe fn resolve_motion(pass: Raw) -> Raw {
    unsafe { rsf_motion_resolve_motion(pass.cast()) }
}

pub(super) unsafe fn resolve_depth(pass: Raw) -> Raw {
    unsafe { rsf_motion_resolve_depth(pass.cast()) }
}

pub(super) unsafe fn resolve_destroy(pass: Raw) {
    unsafe { rsf_motion_resolve_destroy(pass.cast()) };
}

pub(super) unsafe fn fidelity_create(device: Raw) -> Raw {
    let mut pass = null_mut();
    if unsafe { rsf_colour_fidelity_create(device, &mut pass) } == 0 {
        return null_mut();
    }
    pass.cast()
}

pub(super) unsafe fn fidelity_run(
    pass: Raw,
    context: Raw,
    scene: Raw,
    output: Raw,
    depth: Raw,
    inverted: u32,
    jitter: *const f32,
) -> i32 {
    unsafe { rsf_colour_fidelity_run(pass.cast(), context, scene, output, depth, inverted, jitter) }
}

pub(super) unsafe fn fidelity_destroy(pass: Raw) {
    unsafe { rsf_colour_fidelity_destroy(pass.cast()) };
}

pub(super) unsafe fn transport_create(device: Raw) -> Raw {
    let mut pass = null_mut();
    if unsafe { rsf_colour_transport_create(device, &mut pass) } == 0 {
        return null_mut();
    }
    pass.cast()
}

pub(super) unsafe fn transport_encode(
    pass: Raw,
    context: Raw,
    scene: Raw,
    exposure: Raw,
    width: u32,
    height: u32,
    out: *mut Raw,
) -> i32 {
    unsafe {
        rsf_colour_transport_encode(pass.cast(), context, scene, exposure, width, height, out)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn transport_decode(
    pass: Raw,
    context: Raw,
    target: Raw,
    exposure: Raw,
    width: u32,
    height: u32,
    scene: Raw,
    scene_width: u32,
    scene_height: u32,
    jitter: *const f32,
) -> i32 {
    unsafe {
        rsf_colour_transport_decode(
            pass.cast(),
            context,
            target,
            exposure,
            width,
            height,
            scene,
            scene_width,
            scene_height,
            jitter,
        )
    }
}

pub(super) unsafe fn transport_destroy(pass: Raw) {
    unsafe { rsf_colour_transport_destroy(pass.cast()) };
}

#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn sharpen(
    pass: *mut Raw,
    context: Raw,
    output: Raw,
    motion: Raw,
    depth: Raw,
    exposure: Raw,
    request: *const Sharpening,
    frame: *const SharpenFrame,
) -> i32 {
    let request = unsafe { &*request };
    if unsafe { rsf_sharpening_requested(request) } == 0 {
        return 1;
    }
    let inputs = [output, motion, depth, exposure];
    i32::from(
        unsafe { rsf_core_sharpen11_run(&mut *pass, context, &inputs, request, &*frame) } != 0,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn alternate_create(
    device: Raw,
    width: u32,
    height: u32,
    directories: *const *const c_char,
    units: f32,
    standard_depth: u32,
    auto_exposure: u32,
    log: rsf_dlss_pipeline_log_fn,
    user: Raw,
    out: *mut Raw,
) -> i32 {
    let [fsr2, fsr3, fsr4, xess] = unsafe { *directories.cast::<[*const c_char; 4]>() };
    unsafe {
        rsf_sr_legacy_create(
            device,
            width,
            height,
            fsr2,
            fsr3,
            fsr4,
            xess,
            units,
            standard_depth,
            auto_exposure,
            log,
            user,
            out.cast(),
        )
    }
}

pub(super) fn preset_valid(preset: u32) -> i32 {
    i32::from(crate::streamline_sr::preset_valid(preset))
}

pub(super) unsafe fn dump_bytes(
    device: Raw,
    context: Raw,
    texture: Raw,
    prefix: *const c_char,
    view: u32,
    scale: f32,
    log: rsf_dlss_pipeline_log_fn,
) -> i32 {
    let options = TextureOptions::new(
        unsafe { CStr::from_ptr(prefix) },
        view,
        scale,
        Log::new(log, null_mut()),
    );
    unsafe { rsf_dump_texture_bytes(device, context, texture, &options) }
}

pub(super) unsafe fn dump_image(
    device: Raw,
    context: Raw,
    texture: Raw,
    prefix: *const c_char,
    view: u32,
    scale: f32,
    log: rsf_dlss_pipeline_log_fn,
) -> i32 {
    let options = TextureOptions::new(
        unsafe { CStr::from_ptr(prefix) },
        view,
        scale,
        Log::new(log, null_mut()),
    );
    unsafe { rsf_dump_texture(device, context, texture, &options, null_mut()) }
}
