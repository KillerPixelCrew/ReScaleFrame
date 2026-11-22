// SPDX-License-Identifier: MIT
use super::{DlssFrame, Handle, Log, Pass, Platform, Provider, TextureInfo, rsf_fg_frame};
use crate::d3d11_d3d12::*;
use crate::d3d11_present::{rsf_core_d3d11p_graphics, rsf_core_d3d11p_host};
use crate::d3d11_present_abi::rsf_streamline_graphics;
use crate::d3d12_fence::rsf_core_d3d12_wait_fence;
use crate::fg_leases::rsf_d3d12_fence_completed;
use crate::native_fg_cpu::NATIVE_PLATFORM as CPU_PLATFORM;
use crate::native_scene::{rsf_native_scene_classify_present, rsf_native_scene_for_window};
use crate::native_window::rsf_native_window_match;
use crate::shared_surface::{
    ABI_VERSION as SURFACE_ABI, SurfaceSetup, rsf_shared_fence_create, rsf_shared_surface_create,
};
use crate::sys::*;
use rsf_game_sys::rsf_frame_allows_fg;
use std::ffi::c_char;
use std::ptr::null_mut;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_TEXTURE2D_DESC, ID3D11DeviceContext, ID3D11DeviceContext4, ID3D11Texture2D,
};
use windows::core::{IUnknown, Interface};

unsafe extern "C" {
    fn rsf_d3d11_present_has_owner() -> i32;
    fn rsf_d3d11_present_is_owner(chain: Handle) -> i32;
    fn rsf_d3d11_present_provider() -> *const Provider;
    fn rsf_d3d11_present_session() -> Handle;
    fn rsf_d3d11_present_backend() -> u32;
    fn rsf_d3d11_present_generation() -> u64;
    fn rsf_d3d11_present_retire(
        session: Handle,
        list: Handle,
        fence: *mut Handle,
        value: *mut u64,
    ) -> i32;
    fn rsf_d3d11_present_abort(id: u64) -> i32;
    fn rsf_streamline_host_token(host: Handle, source: u64) -> Handle;
    fn rsf_dlss_evaluate(context: Handle, frame: *const DlssFrame) -> i32;
    fn rsf_dlss_evaluate_shared(list: Handle, frame: *const DlssFrame, source: u64) -> i32;
    fn rsf_input_dump_sr(context: Handle, frame: *const DlssFrame, backend: u32);
    fn rsf_input_dump_fg(context: Handle, pass: *const Pass, frame: *const DlssFrame);
    fn rsf_input_dump_fg_prepare(pass: *const Pass, frame: *const rsf_fg_frame, result: i32);
    fn rsf_input_dump_named_texture(
        context: Handle,
        texture: Handle,
        request: *const rsf_named_frame_dump,
        report: *mut rsf_named_frame_dump_report,
    ) -> i32;
}

pub(super) static NATIVE_PLATFORM: Platform = Platform {
    cpu: &CPU_PLATFORM,
    present_has_owner: rsf_d3d11_present_has_owner,
    present_is_owner: rsf_d3d11_present_is_owner,
    present_host: rsf_core_d3d11p_host,
    present_provider: rsf_d3d11_present_provider,
    present_session: rsf_d3d11_present_session,
    present_backend: rsf_d3d11_present_backend,
    present_generation: rsf_d3d11_present_generation,
    present_graphics,
    present_retire: rsf_d3d11_present_retire,
    present_abort: rsf_d3d11_present_abort,
    host_token,
    window_match: rsf_native_window_match,
    scene_for_window: rsf_native_scene_for_window,
    scene_present_path: rsf_native_scene_classify_present,
    allows_fg: rsf_frame_allows_fg,
    context_device,
    query_context4: query::<ID3D11DeviceContext, ID3D11DeviceContext4>,
    query_texture: query::<IUnknown, ID3D11Texture2D>,
    texture_info,
    create_event,
    close_event,
    fence_create,
    fence_d3d11,
    fence_d3d12,
    surface_create,
    surface_destroy,
    surface_d3d11,
    surface_d3d12,
    fence_completed: rsf_d3d12_fence_completed,
    wait_fence,
    flush,
    copy_resource,
    context_signal,
    context_wait,
    create_commands,
    create_output,
    reset_commands,
    close_commands,
    execute,
    transition,
    copy_transitioned,
    queue_signal,
    queue_wait,
    dlss_evaluate: rsf_dlss_evaluate,
    dlss_evaluate_shared: rsf_dlss_evaluate_shared,
    dump_sr: rsf_input_dump_sr,
    dump_fg: rsf_input_dump_fg,
    dump_fg_prepare: rsf_input_dump_fg_prepare,
    dump_named,
};

unsafe extern "C" fn present_graphics(
    device: *mut Handle,
    queue: *mut Handle,
    native: *mut Handle,
) -> i32 {
    let mut graphics = rsf_streamline_graphics {
        struct_size: size_of::<rsf_streamline_graphics>() as u32,
        ..rsf_streamline_graphics::EMPTY
    };
    if unsafe { rsf_core_d3d11p_graphics(&mut graphics) } == 0 {
        return 0;
    }
    unsafe {
        device.write(graphics.device);
        queue.write(graphics.queue);
        native.write(graphics.native_device);
    }
    1
}

unsafe extern "C" fn host_token(host: Handle, source: u64) -> i32 {
    i32::from(!unsafe { rsf_streamline_host_token(host, source) }.is_null())
}

unsafe extern "C" fn context_device(context: Handle) -> Handle {
    unsafe { borrow::<ID3D11DeviceContext>(&context).GetDevice() }
        .map_or(null_mut(), Interface::into_raw)
}

unsafe extern "C" fn texture_info(texture: Handle, out: *mut TextureInfo) -> i32 {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        borrow::<ID3D11Texture2D>(&texture).GetDesc(&mut desc);
        out.write(TextureInfo {
            width: desc.Width,
            height: desc.Height,
            format: desc.Format.0 as u32,
            samples: desc.SampleDesc.Count,
        });
    }
    1
}

unsafe extern "C" fn fence_create(device11: Handle, device12: Handle) -> Handle {
    let mut fence = null_mut();
    if unsafe { rsf_shared_fence_create(device11, device12, None, null_mut(), &mut fence) } == 0 {
        fence.cast()
    } else {
        null_mut()
    }
}

unsafe extern "C" fn surface_create(
    device11: Handle,
    device12: Handle,
    width: u32,
    height: u32,
    format: u32,
) -> Handle {
    let setup = SurfaceSetup {
        struct_size: size_of::<SurfaceSetup>() as u32,
        abi_version: SURFACE_ABI,
        width,
        height,
        format,
        render_target: 0,
        log: None,
        log_user: null_mut(),
        unordered_access: 0,
    };
    let mut surface = null_mut();
    if unsafe { rsf_shared_surface_create(device11, device12, &setup, &mut surface) } == 0 {
        surface.cast()
    } else {
        null_mut()
    }
}

unsafe extern "C" fn wait_fence(fence: Handle, value: u64, event: Handle) -> i32 {
    i32::from(
        unsafe {
            rsf_core_d3d12_wait_fence(
                fence,
                value,
                event,
                10_000,
                rsf_d3d12_fence_completed,
                arm_fence,
            )
        } >= 0,
    )
}

unsafe extern "C" fn dump_named(
    context: Handle,
    texture: Handle,
    directory: *const c_char,
    name: *const c_char,
    view: u32,
    scale: f32,
    log: Log,
    user: Handle,
) -> i32 {
    let request = rsf_named_frame_dump {
        struct_size: size_of::<rsf_named_frame_dump>() as u32,
        abi_version: RSF_NAMED_FRAME_DUMP_ABI_VERSION,
        directory_utf8: directory,
        name,
        view,
        scale,
        log,
        log_user: user,
    };
    unsafe { rsf_input_dump_named_texture(context, texture, &request, null_mut()) }
}
