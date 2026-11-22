// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d11_d3d12::borrow;
use crate::d3d11_output_abi::{rsf_d3d11_output_create_texture, rsf_d3d11_output_texture_desc};
use crate::d3d11_present::rsf_core_d3d11p_host;
use crate::d3d11_sharpening::{
    Frame as SharpenFrame, rsf_core_sharpen11_destroy, rsf_core_sharpen11_run,
};
use crate::graphics_pass_abi::{
    RSF_MOTION_DECODE_ABI_VERSION, rsf_motion_decode_setup as DecodeSetup,
};
use crate::input_dump::{Log, TextureOptions};
use crate::motion_resolve::{
    rsf_motion_resolve_create, rsf_motion_resolve_depth, rsf_motion_resolve_destroy,
    rsf_motion_resolve_motion, rsf_motion_resolve_run,
};
use crate::sr_legacy::rsf_sr_legacy_create;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

unsafe extern "C" {
    fn rsf_dlss_share_host(host: Raw, log: rsf_dlssp_log_fn, user: Raw) -> i32;
    fn rsf_dlss_load(setup: *const rsf_dlss_setup) -> i32;
    fn rsf_dlss_set_device(device: Raw) -> i32;
    fn rsf_dlss_query_support(support: *mut rsf_dlss_support) -> i32;
    fn rsf_dlss_plan_render_size(plan: *mut rsf_dlss_plan) -> i32;
    fn rsf_dlss_release_resources() -> i32;
    fn rsf_dlss_release_viewport(viewport: u32) -> i32;
    fn rsf_dlss_shutdown() -> i32;
    fn rsf_dlss_evaluate(context: Raw, frame: *const DlssFrame) -> i32;
    fn rsf_native_fg_evaluate(context: Raw, frame: *const DlssFrame) -> i32;
    fn rsf_native_fg_capture(context: Raw, frame: *const DlssFrame);
    fn rsf_native_sr_release_resources();
    fn rsf_motion_decode_create(device: Raw, setup: &DecodeSetup, out: &mut Raw) -> i32;
    fn rsf_motion_decode_run(pass: Raw, context: Raw, motion: Raw, params: &DecodeParams) -> i32;
    fn rsf_motion_decode_texture(pass: Raw) -> Raw;
    fn rsf_motion_decode_destroy(pass: Raw);
    fn rsf_colour_fidelity_create(device: Raw, out: &mut Raw) -> i32;
    fn rsf_colour_fidelity_run(
        pass: Raw,
        context: Raw,
        scene: Raw,
        output: Raw,
        depth: Raw,
        inverted: u32,
        jitter: *const f32,
    ) -> i32;
    fn rsf_colour_fidelity_destroy(pass: Raw);
    fn rsf_colour_transport_create(device: Raw, out: &mut Raw) -> i32;
    fn rsf_colour_transport_encode(
        pass: Raw,
        context: Raw,
        scene: Raw,
        exposure: Raw,
        width: u32,
        height: u32,
        out: *mut Raw,
    ) -> i32;
    fn rsf_colour_transport_decode(
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
    ) -> i32;
    fn rsf_colour_transport_destroy(pass: Raw);
    fn rsf_input_dump_texture(
        context: Raw,
        name: *const c_char,
        description: *const c_char,
        texture: Raw,
        view: u32,
        scale: f32,
    );
    fn rsf_sharpening_requested(options: &Sharpening) -> u32;
    fn rsf_dump_texture_bytes(
        device: Raw,
        context: Raw,
        texture: Raw,
        options: &TextureOptions,
    ) -> i32;
    fn rsf_dump_texture(
        device: Raw,
        context: Raw,
        texture: Raw,
        options: *const TextureOptions,
        report: Raw,
    ) -> i32;
}

unsafe extern "C" fn clear_black(device: Raw, texture: Raw) {
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

unsafe extern "C" fn create_zeroed(device: Raw, desc: *const TextureDesc, row_pitch: u32) -> Raw {
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

unsafe extern "C" fn decode_create(
    device: Raw,
    width: u32,
    height: u32,
    format: u32,
    log: rsf_dlssp_log_fn,
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
    unsafe { rsf_motion_decode_create(device, &setup, &mut *out) }
}

unsafe extern "C" fn decode_run(
    pass: Raw,
    context: Raw,
    motion: Raw,
    params: *const DecodeParams,
) -> i32 {
    unsafe { rsf_motion_decode_run(pass, context, motion, &*params) }
}

unsafe extern "C" fn resolve_create(device: Raw, width: u32, height: u32) -> Raw {
    let mut pass = null_mut();
    if unsafe { rsf_motion_resolve_create(device, width, height, &mut pass) } == 0 {
        return null_mut();
    }
    pass.cast()
}

unsafe extern "C" fn resolve_run(
    pass: Raw,
    context: Raw,
    motion: Raw,
    depth: Raw,
    params: *const rsf_motion_resolve_params,
) -> i32 {
    unsafe { rsf_motion_resolve_run(pass.cast(), context, motion, depth, params) }
}

unsafe extern "C" fn resolve_motion(pass: Raw) -> Raw {
    unsafe { rsf_motion_resolve_motion(pass.cast()) }
}

unsafe extern "C" fn resolve_depth(pass: Raw) -> Raw {
    unsafe { rsf_motion_resolve_depth(pass.cast()) }
}

unsafe extern "C" fn resolve_destroy(pass: Raw) {
    unsafe { rsf_motion_resolve_destroy(pass.cast()) };
}

unsafe extern "C" fn fidelity_create(device: Raw) -> Raw {
    let mut pass = null_mut();
    if unsafe { rsf_colour_fidelity_create(device, &mut pass) } == 0 {
        return null_mut();
    }
    pass
}

unsafe extern "C" fn transport_create(device: Raw) -> Raw {
    let mut pass = null_mut();
    if unsafe { rsf_colour_transport_create(device, &mut pass) } == 0 {
        return null_mut();
    }
    pass
}

unsafe extern "C" fn sharpen(
    pass: *mut Raw,
    context: Raw,
    output: Raw,
    motion: Raw,
    depth: Raw,
    exposure: Raw,
    request: *const Sharpening,
    frame: *const rsf_dlssp_sharpen_frame,
) -> i32 {
    let request = unsafe { &*request };
    if unsafe { rsf_sharpening_requested(request) } == 0 {
        return 1;
    }
    let frame = unsafe { &*frame };
    let mapping = SharpenFrame {
        depth_linear: 0,
        hdr: 1,
        exposure: 1.0,
        depth_inverted: frame.depth_inverted,
        motion_x: frame.motion_x,
        motion_y: frame.motion_y,
        near_plane: frame.near_plane,
        far_plane: frame.far_plane,
    };
    let inputs = [output, motion, depth, exposure];
    i32::from(
        unsafe { rsf_core_sharpen11_run(&mut *pass, context, &inputs, request, &mapping) } != 0,
    )
}

unsafe extern "C" fn alternate_create(
    device: Raw,
    width: u32,
    height: u32,
    directories: *const *const c_char,
    units: f32,
    standard_depth: u32,
    auto_exposure: u32,
    log: rsf_dlssp_log_fn,
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

extern "C" fn preset_valid(preset: u32) -> i32 {
    i32::from(crate::streamline_sr::preset_valid(preset))
}

unsafe extern "C" fn dump_bytes(
    device: Raw,
    context: Raw,
    texture: Raw,
    prefix: *const c_char,
    view: u32,
    scale: f32,
    log: rsf_dlssp_log_fn,
) -> i32 {
    let options = TextureOptions::new(
        unsafe { CStr::from_ptr(prefix) },
        view,
        scale,
        Log::new(log, null_mut()),
    );
    unsafe { rsf_dump_texture_bytes(device, context, texture, &options) }
}

unsafe extern "C" fn dump_image(
    device: Raw,
    context: Raw,
    texture: Raw,
    prefix: *const c_char,
    view: u32,
    scale: f32,
    log: rsf_dlssp_log_fn,
) -> i32 {
    let options = TextureOptions::new(
        unsafe { CStr::from_ptr(prefix) },
        view,
        scale,
        Log::new(log, null_mut()),
    );
    unsafe { rsf_dump_texture(device, context, texture, &options, null_mut()) }
}

pub(super) static LEAVES: P = P {
    present_host: rsf_core_d3d11p_host,
    share_host: rsf_dlss_share_host,
    load: rsf_dlss_load,
    set_device: rsf_dlss_set_device,
    query_support: rsf_dlss_query_support,
    plan: rsf_dlss_plan_render_size,
    release_resources: rsf_dlss_release_resources,
    release_viewport: rsf_dlss_release_viewport,
    shutdown: rsf_dlss_shutdown,
    evaluate_layer: rsf_dlss_evaluate,
    preset_valid,
    fg_evaluate: rsf_native_fg_evaluate,
    fg_capture: rsf_native_fg_capture,
    native_sr_release: rsf_native_sr_release_resources,
    texture_desc: rsf_d3d11_output_texture_desc,
    create_texture: rsf_d3d11_output_create_texture,
    clear_black,
    create_zeroed,
    decode_create,
    decode_run,
    decode_texture: rsf_motion_decode_texture,
    decode_destroy: rsf_motion_decode_destroy,
    resolve_create,
    resolve_run,
    resolve_motion,
    resolve_depth,
    resolve_destroy,
    fidelity_create,
    fidelity_run: rsf_colour_fidelity_run,
    fidelity_destroy: rsf_colour_fidelity_destroy,
    transport_create,
    transport_encode: rsf_colour_transport_encode,
    transport_decode: rsf_colour_transport_decode,
    transport_destroy: rsf_colour_transport_destroy,
    sharpen,
    sharpen_destroy: rsf_core_sharpen11_destroy,
    alternate_create,
    input_dump: rsf_input_dump_texture,
    dump_bytes,
    dump_image,
};
