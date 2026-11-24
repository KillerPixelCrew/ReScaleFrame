// SPDX-License-Identifier: MIT
use super::{Handle, Log, TextureInfo};
use crate::d3d11_d3d12::*;
use crate::d3d11_present::rsf_d3d11_present_graphics;
use crate::shared_surface::{
    ABI_VERSION as SURFACE_ABI, SurfaceSetup, rsf_shared_fence_create, rsf_shared_surface_create,
};
use crate::sys::rsf_streamline_graphics;
use crate::sys::*;
use std::ffi::c_char;
use std::ptr::null_mut;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_TEXTURE2D_DESC, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::core::{IUnknown, Interface};

// The native_fg owner calls these directly. Rust owners elsewhere in the Runtime appear under the
// names the owner uses.
pub(super) use crate::d3d11_d3d12::{
    close_commands, close_event, context_signal, context_wait, copy_resource, copy_transitioned,
    create_commands, create_event, create_output, execute, fence_d3d11, fence_d3d12, flush,
    query_context4, queue_signal, queue_wait, reset_commands, surface_d3d11, surface_d3d12,
    surface_destroy, transition,
};
pub(super) use crate::d3d11_present::rsf_d3d11_present_host as present_host;
pub(super) use crate::d3d11_present::{
    rsf_d3d11_present_abort as present_abort, rsf_d3d11_present_backend as present_backend,
    rsf_d3d11_present_generation as present_generation,
    rsf_d3d11_present_has_owner as present_has_owner,
    rsf_d3d11_present_is_owner as present_is_owner, rsf_d3d11_present_provider as present_provider,
    rsf_d3d11_present_retire as present_retire, rsf_d3d11_present_session as present_session,
};
pub(super) use crate::dlss_streamline::exports::{
    rsf_dlss_evaluate as dlss_evaluate, rsf_dlss_evaluate_shared as dlss_evaluate_shared,
};
pub(super) use crate::fg_leases::rsf_d3d12_fence_completed as fence_completed;
use crate::input_dump::abi::rsf_input_dump_named_texture;
pub(super) use crate::input_dump::{
    rsf_input_dump_fg as dump_fg, rsf_input_dump_fg_prepare as dump_fg_prepare,
    rsf_input_dump_sr as dump_sr,
};
pub(super) use crate::native_scene::{
    rsf_native_scene_classify_present as scene_present_path,
    rsf_native_scene_for_window as scene_for_window,
};
pub(super) use crate::native_window::rsf_native_window_match as window_match;
use crate::streamline_host::rsf_streamline_host_token;
pub(super) use rsf_game_sys::rsf_frame_allows_fg as allows_fg;

pub(super) unsafe fn query_texture(object: Handle) -> Handle {
    unsafe { query::<IUnknown, ID3D11Texture2D>(object) }
}

pub(super) unsafe fn present_graphics(
    device: *mut Handle,
    queue: *mut Handle,
    native: *mut Handle,
) -> i32 {
    let mut graphics = rsf_streamline_graphics {
        struct_size: size_of::<rsf_streamline_graphics>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    if unsafe { rsf_d3d11_present_graphics(&mut graphics) } == 0 {
        return 0;
    }
    unsafe {
        device.write(graphics.device);
        queue.write(graphics.queue);
        native.write(graphics.native_device);
    }
    1
}

pub(super) unsafe fn host_token(host: Handle, source: u64) -> i32 {
    i32::from(!unsafe { rsf_streamline_host_token(host.cast(), source) }.is_null())
}

pub(super) unsafe fn context_device(context: Handle) -> Handle {
    unsafe { borrow::<ID3D11DeviceContext>(&context).GetDevice() }
        .map_or(null_mut(), Interface::into_raw)
}

pub(super) unsafe fn texture_info(texture: Handle, out: *mut TextureInfo) -> i32 {
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

pub(super) unsafe fn fence_create(device11: Handle, device12: Handle) -> Handle {
    let mut fence = null_mut();
    if unsafe { rsf_shared_fence_create(device11, device12, None, null_mut(), &mut fence) } == 0 {
        fence.cast()
    } else {
        null_mut()
    }
}

pub(super) unsafe fn surface_create(
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

pub(super) unsafe fn wait_fence(fence: Handle, value: u64, event: Handle) -> i32 {
    i32::from(unsafe { crate::d3d12_fence::wait_fence(fence, value, event, 10_000) } >= 0)
}

#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn dump_named(
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
