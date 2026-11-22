// SPDX-License-Identifier: MIT
use super::*;
use crate::overlay_service::{settings_apply, settings_get};
use crate::settings_store::{
    rsf_settings_store_read_fg_debug, rsf_settings_store_write_fg_debug, set_sharpening,
    sharpening_of,
};
use crate::sys::{
    RSF_FG_BACKEND_ALL, RSF_FG_FIXED, RSF_REFLEX_BOOST, RSF_RENDER_FRONTEND_ABI_VERSION,
    rsf_render_settings,
};
use crate::sys::{
    RSF_OVERLAY_API_VERSION, RSF_OVERLAY_SERVICE_OPTION_TOGGLE, RSF_OVERLAY_SR_INPUT_COLOR,
    RSF_OVERLAY_SR_INPUT_DEPTH, RSF_OVERLAY_SR_INPUT_MOTION, rsf_overlay_service_option,
    rsf_overlay_service_setup, rsf_overlay_sr_report,
};
use std::os::windows::ffi::OsStringExt;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicU64};

pub(super) struct Managed {
    path: Vec<u16>,
    section: Vec<u16>,
    debug: AtomicU32,
    revision: AtomicU64,
    report: Mutex<rsf_overlay_sr_report>,
}

pub(super) type SettingsSlot = OnceLock<Managed>;

fn interval(fps: u32) -> u32 {
    1_000_000u32.checked_div(fps).unwrap_or(0)
}

fn mode(enabled: bool) -> u32 {
    if enabled { RSF_FG_FIXED } else { RSF_FG_OFF }
}

unsafe fn wide(text: *const u16) -> Vec<u16> {
    let length = (0..).find(|&i| unsafe { *text.add(i) == 0 }).unwrap();
    unsafe { std::slice::from_raw_parts(text, length + 1) }.to_vec()
}

fn binding(user: Handle) -> &'static Pipeline {
    // The GL owner remains allocated after close and the overlay borrows this
    // identity for its process lifetime.
    unsafe { &*user.cast::<Pipeline>() }
}

unsafe extern "C" fn log(user: Handle, message: *const c_char) {
    let shared = binding(user).shared;
    unsafe { hook_log(ptr::from_ref(shared).cast_mut().cast(), message) };
}

unsafe extern "C" fn accept(user: Handle, window: Handle) -> i32 {
    let pipeline = binding(user);
    i32::from(pipeline.ready.load(Ordering::Acquire) && pipeline.parent == window)
}

unsafe extern "C" fn report(user: Handle, out: *mut rsf_overlay_sr_report) -> i32 {
    if !unsafe { has_capacity(out) } {
        return 0;
    }
    let Some(settings) = binding(user).settings.get() else {
        return 0;
    };
    unsafe { out.write(*settings.report.lock().unwrap()) };
    1
}

unsafe extern "C" fn changed(user: Handle, _: *const rsf_render_settings) {
    if let Some(settings) = binding(user).settings.get() {
        settings.revision.fetch_add(1, Ordering::Release);
    }
}

unsafe extern "C" fn options_read(user: Handle, values: *mut i32, count: u32) {
    if count != 0
        && !values.is_null()
        && let Some(settings) = binding(user).settings.get()
    {
        unsafe { values.write(settings.debug.load(Ordering::Acquire) as i32) };
    }
}

unsafe extern "C" fn option_changed(user: Handle, index: u32, value: i32) -> i32 {
    let Some(settings) = binding(user).settings.get().filter(|_| index == 0) else {
        return 0;
    };
    let value = u32::from(value != 0);
    let result = unsafe {
        rsf_settings_store_write_fg_debug(settings.path.as_ptr(), settings.section.as_ptr(), value)
    };
    if result != OK {
        return 0;
    }
    settings.debug.store(value, Ordering::Release);
    settings.revision.fetch_add(1, Ordering::Release);
    1
}

pub(super) fn present(
    pipeline: &Pipeline,
    state: &State,
    chain: Handle,
    buffer: Handle,
    frame: u64,
) {
    let (Some(settings), Some(active)) = (pipeline.settings.get(), state.active.as_ref()) else {
        return;
    };
    let active_sr = state.evaluated_frame == frame && state.status.effective_sr != SR_NONE;
    *settings.report.lock().unwrap() = rsf_overlay_sr_report {
        struct_size: size_of::<rsf_overlay_sr_report>() as u32,
        effective_backend: if active_sr {
            state.status.effective_sr
        } else {
            SR_NONE
        },
        effective_quality: active.shape.quality,
        render_width: active.render_width,
        render_height: active.render_height,
        output_width: state.width,
        output_height: state.height,
        frames_evaluated: state.status.evaluated as u32,
        last_result: if state.status.sr_result == OK {
            state.evaluation_result
        } else {
            state.status.sr_result
        },
        inputs: if active_sr {
            RSF_OVERLAY_SR_INPUT_COLOR | RSF_OVERLAY_SR_INPUT_DEPTH | RSF_OVERLAY_SR_INPUT_MOTION
        } else {
            0
        },
        ..Default::default()
    };
    unsafe {
        (pipeline.platform.overlay_present)(
            state.device,
            state.queue,
            chain,
            buffer,
            pipeline.parent,
            Some(hook_log),
            ptr::from_ref(pipeline.shared).cast_mut().cast(),
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_settings_open(
    pipeline: *mut Pipeline,
    path: *const u16,
    section: *const u16,
    identity: *const c_char,
    legacy: *const rsf_gl_pipeline_options,
    limit_fps: u32,
) -> i32 {
    let Some(legacy) = (unsafe { read_sized(legacy) }) else {
        return INVALID;
    };
    if path.is_null()
        || section.is_null()
        || identity.is_null()
        || unsafe { *path == 0 || *section == 0 }
    {
        return INVALID;
    }
    with_state(pipeline, |pipeline, state| {
        if state.overlay {
            return OK;
        }
        let path = unsafe { wide(path) };
        let section = unsafe { wide(section) };
        let file = std::path::PathBuf::from(std::ffi::OsString::from_wide(&path[..path.len() - 1]));
        let migrate = std::fs::metadata(file)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
        let debug = u32::from(
            unsafe {
                rsf_settings_store_read_fg_debug(
                    path.as_ptr(),
                    section.as_ptr(),
                    legacy.generation.debug,
                )
            } != 0,
        );
        let settings = pipeline.settings.get_or_init(|| Managed {
            path,
            section,
            debug: AtomicU32::new(debug),
            revision: AtomicU64::new(0),
            report: Mutex::new(rsf_overlay_sr_report::default()),
        });
        let mut defaults = rsf_render_settings {
            struct_size: size_of::<rsf_render_settings>() as u32,
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            sr_enabled: u32::from(legacy.sr_backend != SR_NONE),
            sr_backend: if legacy.sr_backend != SR_NONE {
                legacy.sr_backend
            } else {
                SR_AUTO
            },
            sr_quality: legacy.quality,
            dlss_preset: legacy.dlss_preset,
            fg_enabled: u32::from(legacy.fg_backend != 0),
            fg_backend: if legacy.fg_backend != 0 {
                legacy.fg_backend
            } else {
                RSF_FG_BACKEND_AUTO
            },
            generated_frames: legacy.generation.generated_frames,
            latency: legacy.generation.reflex_mode,
            frame_limit_us: interval(limit_fps),
            ..unsafe { std::mem::zeroed() }
        };
        let sharpening = if legacy.sharpening.struct_size != 0 {
            legacy.sharpening
        } else {
            sharpening_policy::defaults()
        };
        set_sharpening(&mut defaults, &sharpening);
        let option = rsf_overlay_service_option {
            label_utf8: c"Frame generation debug".as_ptr(),
            kind: RSF_OVERLAY_SERVICE_OPTION_TOGGLE,
            value: debug as i32,
            minimum: 0,
            maximum: 1,
            step: 1,
            divisor: 1,
            restart: 0,
        };
        let setup = rsf_overlay_service_setup {
            struct_size: size_of::<rsf_overlay_service_setup>() as u32,
            abi_version: RSF_OVERLAY_API_VERSION,
            identity_utf8: identity,
            configuration_path: settings.path.as_ptr(),
            configuration_section: settings.section.as_ptr(),
            defaults,
            sr_backend_choices: SR_CHOICES | (1 << SR_AUTO),
            sr_quality_choices: 0x1f,
            fg_backend_choices: RSF_FG_BACKEND_ALL | (1 << RSF_FG_BACKEND_AUTO),
            latency_choices: (1 << (RSF_REFLEX_BOOST + 1)) - 1,
            max_generated_frames: 15,
            dlss_preset_available: 1,
            log: Some(log),
            accept_window: Some(accept),
            sr_report: Some(report),
            settings_changed: Some(changed),
            user: ptr::from_ref(pipeline).cast_mut().cast(),
            option_count: 1,
            options: &option,
            options_read: Some(options_read),
            option_changed: Some(option_changed),
            option_label: None,
        };
        if unsafe { (pipeline.platform.overlay_configure)(&setup) } != OK {
            return INIT_FAILED;
        }
        state.overlay = true;
        settings.revision.store(1, Ordering::Release);
        if migrate {
            let saved = unsafe { settings_apply(&defaults) };
            let debug_saved = unsafe {
                rsf_settings_store_write_fg_debug(
                    settings.path.as_ptr(),
                    settings.section.as_ptr(),
                    debug,
                )
            };
            if saved != OK || debug_saved != OK {
                pipeline
                    .log("OpenGL: legacy settings could not be saved to the shared configuration.");
            }
        }
        OK
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_settings_revision(pipeline: *mut Pipeline) -> u64 {
    unsafe { pipeline.as_ref() }
        .filter(|pipeline| pipeline.ready.load(Ordering::Acquire))
        .and_then(|pipeline| pipeline.settings.get())
        .map_or(0, |settings| settings.revision.load(Ordering::Acquire))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_configure_settings(
    pipeline: *mut Pipeline,
    width: u32,
    height: u32,
) -> i32 {
    let Some(settings) =
        (unsafe { pipeline.as_ref() }).and_then(|pipeline| pipeline.settings.get())
    else {
        return NOT_READY;
    };
    let mut requested = rsf_render_settings {
        struct_size: size_of::<rsf_render_settings>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        ..unsafe { std::mem::zeroed() }
    };
    if settings.revision.load(Ordering::Acquire) == 0
        || unsafe { settings_get(&mut requested) } != OK
    {
        return NOT_READY;
    }
    let options = rsf_gl_pipeline_options {
        struct_size: size_of::<rsf_gl_pipeline_options>() as u32,
        sr_backend: if requested.sr_enabled != 0 {
            requested.sr_backend
        } else {
            SR_NONE
        },
        quality: requested.sr_quality,
        fg_backend: if requested.fg_enabled != 0 {
            requested.fg_backend
        } else {
            0
        },
        generation: rsf_native_fg_options {
            struct_size: size_of::<rsf_native_fg_options>() as u32,
            mode: mode(requested.fg_enabled != 0),
            generated_frames: requested.generated_frames,
            reflex_mode: requested.latency,
            frame_limit_us: requested.frame_limit_us,
            debug: settings.debug.load(Ordering::Acquire),
        },
        dlss_preset: requested.dlss_preset,
        sharpening: sharpening_of(&requested, &sharpening_policy::defaults()),
    };
    unsafe { rsf_gl_pipeline_configure(pipeline, &options, width, height) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_configure_legacy(
    pipeline: *mut Pipeline,
    options: *const rsf_gl_pipeline_options,
    limit_fps: u32,
    width: u32,
    height: u32,
) -> i32 {
    let Some(mut options) = (unsafe { read_sized(options) }) else {
        return INVALID;
    };
    options.generation.mode = mode(options.fg_backend != 0);
    options.generation.frame_limit_us = interval(limit_fps);
    unsafe { rsf_gl_pipeline_configure(pipeline, &options, width, height) }
}
