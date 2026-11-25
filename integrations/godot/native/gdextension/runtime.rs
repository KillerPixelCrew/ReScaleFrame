// SPDX-License-Identifier: MIT
//! `ReScaleFrameRuntime`: a `RefCounted` with no state of its own. `configure` reads the option
//! Dictionary into typed settings for the engine module; `get_status` reports the renderer
//! admission, the requested settings and the status record.
use super::api::{self, Bind, Godot, StringName, Variant};
use super::sys::*;
use super::{Dictionary, RUNTIME_CLASS, Sink};
use crate::control::{
    default_settings, rsfg_reset_history, rsfg_settings, rsfg_status, status_header,
};
use crate::sys::{rsf_input_dump_request, rsf_sharpening_core_defaults};
use crate::window::rsfg_request_latency_ping;
use std::ffi::{CStr, c_void};
use std::ptr::NonNull;

/// `GDExtensionClassCreateInstance3`. The class has no fields, so the instance is a non-null
/// marker the engine hands back to `free`.
pub(super) unsafe extern "C" fn create(_: *mut c_void, notify_postinitialize: GdBool) -> ObjectPtr {
    let Some(godot) = api::godot() else {
        return std::ptr::null_mut();
    };
    let object = godot.construct(c"RefCounted");
    if object.is_null() {
        return object;
    }
    let class = StringName::new(godot, RUNTIME_CLASS);
    let marker = NonNull::<u8>::dangling().as_ptr().cast();
    unsafe { (godot.interface.object_set_instance)(object, class.ptr(), marker) };
    if notify_postinitialize != 0 {
        godot.postinitialize(object);
    }
    object
}

pub(super) unsafe extern "C" fn free(_: *mut c_void, _: *mut c_void) {}

/// Forward+ on Direct3D 12, or on Vulkan when this build carries the Vulkan SR payload: whether
/// the driver and the renderer are supported. A missing RenderingServer supports neither.
fn admission(godot: &'static Godot, rendering: ObjectPtr) -> (bool, bool) {
    if rendering.is_null() {
        return (false, false);
    }
    let name = |method| unsafe { godot.string(method, rendering, &[]) }.map(|name| name.utf8());
    let driver = match name(Bind::GET_RENDERING_DRIVER_NAME).as_deref() {
        Some(b"d3d12") => true,
        Some(b"vulkan") => (unsafe { crate::sys::rsf_vkhost_supported() }) != 0,
        _ => false,
    };
    let renderer = name(Bind::GET_RENDERING_METHOD).as_deref() == Some(b"forward_plus".as_slice());
    (driver, renderer)
}

/// `configure(options, viewport)`: absent options keep the defaults; the scalar options must be
/// an INT or BOOL within 0..=u32::MAX and truncate to 32 bits, the sharpening options convert by
/// Godot's Variant rules unchecked.
///
/// # Safety
/// `options` is a live Dictionary.
pub(super) unsafe fn configure(
    godot: &'static Godot,
    options: ConstTypePtr,
    viewport: u64,
) -> bool {
    if viewport == 0
        || options.is_null()
        || admission(godot, godot.rendering_server()) != (true, true)
    {
        return false;
    }
    let get = |name: &CStr| {
        let key = Variant::string(godot, name);
        unsafe { api::dictionary_get(godot, options, &key) }
    };
    let integer = |name: &CStr, fallback: u32| {
        get(name).map_or(fallback, |raw| {
            let mut value = 0i64;
            unsafe { (godot.to_int)((&raw mut value).cast(), raw) };
            value as u32
        })
    };
    let real = |name: &CStr, fallback: f32| {
        get(name).map_or(fallback, |raw| {
            let mut value = 0f64;
            unsafe { (godot.to_float)((&raw mut value).cast(), raw) };
            value as f32
        })
    };
    const CHECKED: [&CStr; 9] = [
        c"sr_backend",
        c"quality",
        c"dlss_preset",
        c"fg_backend",
        c"fg_mode",
        c"generated_frames",
        c"latency_mode",
        c"frame_limit_us",
        c"gameplay",
    ];
    for name in CHECKED {
        let Some(raw) = get(name) else { continue };
        if !matches!(
            unsafe { (godot.interface.variant_get_type)(raw) },
            TYPE_INT | TYPE_BOOL
        ) {
            return false;
        }
        let mut value = 0i64;
        unsafe { (godot.to_int)((&raw mut value).cast(), raw) };
        if !(0..=i64::from(u32::MAX)).contains(&value) {
            return false;
        }
    }
    let mut value = default_settings();
    value.sr_backend = integer(c"sr_backend", value.sr_backend);
    value.quality = integer(c"quality", value.quality);
    value.dlss_preset = integer(c"dlss_preset", value.dlss_preset);
    value.fg_backend = integer(c"fg_backend", value.fg_backend);
    value.fg_mode = integer(c"fg_mode", value.fg_mode);
    value.generated_frames = integer(c"generated_frames", value.generated_frames);
    value.latency_mode = integer(c"latency_mode", value.latency_mode);
    value.frame_limit_us = integer(c"frame_limit_us", value.frame_limit_us);
    value.gameplay = integer(c"gameplay", value.gameplay);
    let mut sharpening = unsafe { rsf_sharpening_core_defaults() };
    sharpening.mode = integer(c"sharpening_mode", sharpening.mode);
    sharpening.sharpness = real(c"sharpening_sharpness", sharpening.sharpness);
    sharpening.mas = integer(c"sharpening_mas", sharpening.mas);
    sharpening.motion_sharpness = real(c"sharpening_motion_sharpness", sharpening.motion_sharpness);
    sharpening.motion_threshold = real(c"sharpening_motion_threshold", sharpening.motion_threshold);
    sharpening.motion_scale_limit = real(
        c"sharpening_motion_scale_limit",
        sharpening.motion_scale_limit,
    );
    sharpening.contrast_enabled =
        integer(c"sharpening_contrast_enabled", sharpening.contrast_enabled);
    sharpening.contrast = real(c"sharpening_contrast", sharpening.contrast);
    sharpening.depth_scale = real(c"sharpening_depth_scale", sharpening.depth_scale);
    sharpening.depth_bias = real(c"sharpening_depth_bias", sharpening.depth_bias);
    sharpening.clamp_output = integer(c"sharpening_clamp_output", sharpening.clamp_output);
    sharpening.debug = integer(c"sharpening_debug", sharpening.debug);
    value.sharpening = sharpening;
    crate::control::configure_engine(&value, viewport) != 0
}

/// `get_status()`.
pub(super) fn get_status(godot: &'static Godot) -> Dictionary {
    let mut dictionary = Dictionary::new(godot);
    let rendering = godot.rendering_server();
    let (driver, renderer) = admission(godot, rendering);
    let mut sink = Sink {
        godot,
        dictionary: &mut dictionary,
        rendering,
    };
    sink.name(c"graphics_driver", Bind::GET_RENDERING_DRIVER_NAME);
    sink.name(c"renderer", Bind::GET_RENDERING_METHOD);
    sink.boolean(c"graphics_driver_supported", driver);
    sink.boolean(c"renderer_supported", renderer);
    let mut status = status_header();
    let available = unsafe { rsfg_status(&mut status) } != 0 && status.session_id != 0;
    sink.boolean(c"engine_bridge_ready", available);
    if !available {
        return dictionary;
    }
    let mut requested = default_settings();
    if unsafe { rsfg_settings(&mut requested) } != 0 {
        sink.integer(c"requested_dlss_preset", requested.dlss_preset.into());
        sink.integer(c"requested_fg_backend", requested.fg_backend.into());
        sink.integer(c"requested_fg_mode", requested.fg_mode.into());
        sink.integer(
            c"requested_generated_frames",
            requested.generated_frames.into(),
        );
        sink.integer(c"requested_latency_mode", requested.latency_mode.into());
    }
    macro_rules! fields {
        ($($field:ident),* $(,)?) => {
            $(sink.integer(
                CStr::from_bytes_with_nul(concat!(stringify!($field), "\0").as_bytes()).unwrap(),
                status.$field as i64,
            );)*
        };
    }
    fields!(
        session_id,
        requested_sr,
        active_sr,
        render_width,
        render_height,
        output_width,
        output_height,
        sr_result,
        fg_result,
        fg_backend,
        fg_available,
        fg_active,
        max_generated_frames,
        generated_frames,
        latency_available,
        latency_mode,
        reason,
        pcl_available,
        latency_report_available,
        evaluated_frames,
        refused_frames,
        tagged_frames,
        sleep_calls,
        marker_calls,
        total_presented,
        generated_presented,
        presentation_statistics_valid,
        presentation_statistics_flags,
        source_frame_id,
        provider_generation,
        generated_callbacks,
        window_foreground,
        window_visible,
        foreground_process_id,
        latency_owner,
        input_substitutes,
        order_violations,
        derived_boundaries,
        keyboard_inputs,
        mouse_inputs,
        controller_inputs,
        pcl_pings,
    );
    dictionary
}

pub(super) fn reset_history() {
    rsfg_reset_history();
}

pub(super) fn request_latency_ping() -> bool {
    rsfg_request_latency_ping() != 0
}

/// `request_frame_dump()`: the Runtime capture request runs on the render thread.
pub(super) fn request_frame_dump(godot: &'static Godot) {
    let rendering = godot.rendering_server();
    if rendering.is_null() {
        return;
    }
    let callable = api::Callable::custom(godot, capture, no_arguments);
    unsafe { godot.call(Bind::CALL_ON_RENDER_THREAD, rendering, &[callable.ptr()]) };
}

unsafe extern "C" fn capture(
    _: *mut c_void,
    _: *const *const c_void,
    count: GdInt,
    _: *mut c_void,
    error: *mut CallError,
) {
    let Some(error) = (unsafe { error.as_mut() }) else {
        return;
    };
    if count != 0 {
        *error = CallError {
            error: CALL_ERROR_TOO_MANY_ARGUMENTS,
            argument: 0,
            expected: 0,
        };
        return;
    }
    *error = CallError {
        error: CALL_OK,
        argument: 0,
        expected: 0,
    };
    // `ReScaleFrameRuntime.request_frame_dump`, on the render thread.
    unsafe { rsf_input_dump_request() };
}

unsafe extern "C" fn no_arguments(_: *mut c_void, valid: *mut GdBool) -> GdInt {
    if let Some(valid) = unsafe { valid.as_mut() } {
        *valid = 1;
    }
    0
}
