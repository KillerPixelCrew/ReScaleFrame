// SPDX-License-Identifier: MIT
//! The host's production leaves: Windows calls, the guarded C boundaries and the Rust owners.
//! `d3d11_host` calls them as `native::*`; its unit tests swap in fakes of the same names.
use crate::auxiliary_sr::AuxiliarySr;
use crate::d3d11_device_identity::identity;
use crate::d3d11_host_abi::{DUMP_DENSE_MOTION, DUMP_DEPTH, DUMP_HDR, DUMP_RAW, Pass};
use crate::d3d11_host_reduced::Products;
use crate::native_cpu::rsf_native_cpu_dispatch;
use crate::native_fg_cpu::platform::rsf_native_fg_cpu;
use crate::sys::rsf_observer_present_event;
use crate::sys::rsf_streamline_host;
use crate::sys::{rsf_d3d11h_guard_dump_inputs_start, rsf_d3d11h_guard_dump_texture};
use rsf_game_sys::{rsf_frame_record, rsf_game_cpu_event};
use std::ffi::{c_char, c_void};
use std::ptr::{NonNull, null_mut};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::{IDXGIDeviceSubObject, IDXGISwapChain};
use windows::core::{IUnknown, Interface};

pub(super) use crate::d3d11_output_platform::{
    rsf_d3d11_output_child_device as child_device, rsf_d3d11_output_device_ok as device_ok,
    rsf_d3d11_output_is_immediate as is_immediate,
};
pub(super) use crate::dlss_pipeline::exports::{
    rsf_dlss_pipeline_get_status as pipeline_status,
    rsf_dlss_pipeline_output_texture as pipeline_output,
    rsf_dlss_pipeline_resize_output as pipeline_resize,
    rsf_dlss_pipeline_select_backend as pipeline_select,
    rsf_dlss_pipeline_set_colour_correction as pipeline_colour_correction,
    rsf_dlss_pipeline_set_colour_transport as pipeline_colour_transport,
    rsf_dlss_pipeline_set_preset as pipeline_preset,
    rsf_dlss_pipeline_set_quality as pipeline_quality,
    rsf_dlss_pipeline_start_backend as pipeline_start,
};
pub(super) use crate::dlss_pipeline::rsf_core_dlss_pipeline_apply_sharpening as pipeline_sharpen;
pub(super) use crate::dlss_streamline::exports::rsf_dlss_native_present as dlss_native_present;
pub(super) use crate::native_fg::{
    rsf_native_fg_final as fg_final, rsf_native_fg_options_set as fg_options_set,
    rsf_native_fg_retire as fg_retire, rsf_native_fg_set_log as fg_set_log,
};
pub(super) use crate::native_fg_cpu::platform::{
    rsf_native_fg_frame as fg_frame, rsf_native_fg_rendered_source_set as fg_rendered_source_set,
    rsf_native_fg_submission as fg_submission,
};
pub(super) use crate::native_scene::{
    rsf_native_scene_pass as scene_pass, rsf_native_scene_present_path as scene_present_path,
};
pub(super) use crate::native_sr::{
    rsf_native_sr_capture_fg as fg_capture, rsf_native_sr_evaluate_ordered as sr_evaluate,
};
pub(super) use crate::native_window::{
    rsf_native_window_last_present as window_last_present, rsf_native_window_match as window_match,
    rsf_native_window_present as window_present, rsf_native_window_scope as window_scope,
};
pub(super) use crate::overlay_input::rsf_overlay_input_poll_toggle as overlay_poll_toggle;
pub(super) use crate::settings_store::{
    rsf_settings_store_read_integer as ini_read, rsf_settings_store_write_integer as ini_write,
};
pub(super) use crate::sys::{
    rsf_d3d11h_guard_chain_dispatch as chain_dispatch,
    rsf_d3d11h_guard_creation_begin as creation_begin,
    rsf_d3d11h_guard_creation_end as creation_end, rsf_d3d11h_guard_dump_begin as dump_begin,
    rsf_d3d11h_guard_dump_complete as dump_complete,
    rsf_d3d11h_guard_dump_configure as dump_configure, rsf_d3d11h_guard_dump_pass as dump_pass,
    rsf_d3d11h_guard_dump_poll as dump_poll, rsf_d3d11h_guard_dump_request as dump_request,
    rsf_d3d11h_guard_dump_requested as dump_requested, rsf_d3d11h_guard_dump_result as dump_result,
    rsf_d3d11h_guard_dump_wants as dump_wants,
    rsf_d3d11h_guard_observer_install as observer_install,
    rsf_d3d11h_guard_overlay_configure as overlay_configure,
    rsf_d3d11h_guard_overlay_present as overlay_present,
    rsf_d3d11h_guard_present_install as present_install,
    rsf_d3d11h_guard_present_install_scoped as present_install_scoped,
    rsf_d3d11h_guard_present_is_owner as present_is_owner,
    rsf_d3d11h_guard_present_prepare as present_prepare,
    rsf_d3d11h_guard_wrap_native as wrap_native,
};

type Raw = *mut c_void;

pub(super) fn auxiliary_products() -> Option<Box<dyn Products>> {
    let owner = NonNull::new(crate::allocate_handle(AuxiliarySr::new()))?;
    Some(unsafe { Box::from_raw(owner.as_ptr()) })
}

pub(super) unsafe extern "C" fn fg_prepare(
    user: Raw,
    chain: Raw,
    context: Raw,
    list: Raw,
    buffer: Raw,
    host: *mut rsf_streamline_host,
    session: Raw,
    sync: u32,
) {
    unsafe {
        crate::native_fg::rsf_native_fg_prepare(
            user,
            chain,
            context,
            list,
            buffer,
            host.cast(),
            session,
            sync,
        )
    };
}

pub(super) unsafe extern "C" fn fg_latency_event(_: Raw, event: *const rsf_observer_present_event) {
    unsafe { crate::native_fg::rsf_native_fg_present(event) };
}

pub(super) fn latency_window_bind(session_id: u64, window: Raw) {
    let _ = crate::native_fg_cpu::bind_window(session_id, window.addr() as u64);
}

unsafe extern "C" fn forward_fg(_: Raw, event: *const rsf_game_cpu_event) {
    unsafe { rsf_native_fg_cpu(event) };
}

pub(super) unsafe fn cpu_dispatch_fg(event: *const rsf_game_cpu_event) -> i32 {
    unsafe { rsf_native_cpu_dispatch(event, Some(forward_fg), null_mut()) }
}

pub(super) unsafe fn cpu_event(event: *const rsf_game_cpu_event) {
    let _ = unsafe { crate::native_cpu::rsf_native_cpu_event(event) };
}

pub(super) unsafe fn dump_inputs(
    context: Raw,
    pass: *const Pass,
    record: *const rsf_frame_record,
    original: Raw,
) {
    if unsafe { rsf_d3d11h_guard_dump_inputs_start(context, pass) } == 0 {
        return;
    }
    let pass = unsafe { &*pass };
    let colour = if original.is_null() {
        pass.color_input
    } else {
        original
    };
    for (name, description, texture, view, scale) in [
        (
            c"native_color",
            c"Native pre-tonemap scene colour",
            colour,
            DUMP_HDR,
            1.0,
        ),
        (
            c"native_temporal_mask",
            c"Native numeric temporal rejection mask before weighted normalization",
            pass.temporal_mask,
            DUMP_RAW,
            1.0,
        ),
        (
            c"native_depth",
            c"Native scene device depth",
            pass.depth,
            DUMP_DEPTH,
            1.0,
        ),
        (
            c"native_motion",
            c"Native motion in adapter-declared encoding",
            pass.motion,
            DUMP_DENSE_MOTION,
            8.0,
        ),
    ] {
        if view == DUMP_RAW && texture.is_null() {
            continue;
        }
        // A capture refusal leaves the rest of the batch eligible. A native
        // exception stopped the old batch, so the guard reports it separately.
        if unsafe {
            rsf_d3d11h_guard_dump_texture(
                context,
                record,
                name.as_ptr(),
                description.as_ptr(),
                texture,
                view,
                scale,
            )
        } < 0
        {
            break;
        }
    }
}

pub(super) unsafe fn dump_texture(
    context: Raw,
    record: *const rsf_frame_record,
    name: *const c_char,
    description: *const c_char,
    texture: Raw,
    view: u32,
    scale: f32,
) -> i32 {
    unsafe {
        rsf_d3d11h_guard_dump_texture(context, record, name, description, texture, view, scale)
    }
    .max(0)
}

pub(super) unsafe fn same_object(left: Raw, right: Raw) -> i32 {
    if right.is_null() {
        return 0;
    }
    let Some(left) = (unsafe { identity(left) }) else {
        return 0;
    };
    i32::from(unsafe { identity(right) }.is_some_and(|right| left.as_raw() == right.as_raw()))
}

pub(super) unsafe fn immediate_context(device: Raw) -> Raw {
    unsafe { ID3D11Device::from_raw_borrowed(&device) }
        .and_then(|device| unsafe { device.GetImmediateContext() }.ok())
        .map_or(null_mut(), Interface::into_raw)
}

pub(super) unsafe fn device_vendor(device: Raw, vendor: *mut u32) -> i32 {
    let Some(id) = (unsafe { crate::d3d11_device_identity::device_vendor(device) }) else {
        return 0;
    };
    unsafe { vendor.write(id) };
    1
}

pub(super) unsafe fn chain_device(chain: Raw) -> Raw {
    unsafe { IDXGIDeviceSubObject::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetDevice::<ID3D11Device>() }.ok())
        .map_or(null_mut(), Interface::into_raw)
}

pub(super) unsafe fn chain_window(chain: Raw) -> Raw {
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetDesc() }.ok())
        .map_or(null_mut(), |desc| desc.OutputWindow.0)
}

pub(super) unsafe fn chain_buffer(chain: Raw) -> Raw {
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }.ok())
        .map_or(null_mut(), Interface::into_raw)
}

pub(super) unsafe fn com_identity(object: Raw) -> Raw {
    unsafe { identity(object) }.map_or(null_mut(), Interface::into_raw)
}

pub(super) unsafe fn chain_present(chain: Raw) -> Raw {
    // Log the entry from this receiver's own vtable, without dispatching it.
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .map_or(null_mut(), |chain| chain.vtable().Present as Raw)
}

pub(super) unsafe fn com_release(object: Raw) -> u32 {
    let Some(object) = (unsafe { IUnknown::from_raw_borrowed(&object) }) else {
        return 0;
    };
    unsafe { (object.vtable().Release)(object.as_raw()) }
}
