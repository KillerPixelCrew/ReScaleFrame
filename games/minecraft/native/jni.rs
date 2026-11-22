// SPDX-License-Identifier: MIT
//! Typed JNI entries for org.kpc.rescaleframecraft.render.NativeBridge.

use crate::frame::{self, Configuration};
use crate::observer;
use crate::platform_abi::{
    rsf_gl_pipeline, rsf_gl_pipeline_abort, rsf_gl_pipeline_close,
    rsf_gl_pipeline_configure_legacy, rsf_gl_pipeline_configure_settings, rsf_gl_pipeline_cpu,
    rsf_gl_pipeline_evaluate, rsf_gl_pipeline_get_surfaces, rsf_gl_pipeline_hudless,
    rsf_gl_pipeline_input, rsf_gl_pipeline_open, rsf_gl_pipeline_present,
    rsf_gl_pipeline_settings_open, rsf_gl_pipeline_settings_revision, rsf_gl_pipeline_setup,
    rsf_gl_pipeline_status, rsf_gl_pipeline_status_get, rsf_gl_pipeline_surfaces,
    rsf_gl_window_create, rsf_gl_window_destroy, rsf_input_dump_configure,
    rsf_native_latency_status, rsf_native_latency_status_get,
};
use crate::runtime_directory;
use crate::status::{STATUS_MINIMUM, status_values};
use jni_sys::{
    JNIEnv, JNINativeInterface_, jboolean, jclass, jfloatArray, jint, jintArray, jlong, jlongArray,
    jobject, jstring,
};
use rsf_game_sys::rsf_game_render_pass;
use std::ffi::{CStr, c_char, c_void};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering::SeqCst};
use std::sync::{Mutex, PoisonError};

const OK: i32 = 0;
const ENGINE_VERSION: &CStr = c"Minecraft 1.21.1";
const PROJECT_ID: &CStr = c"11e3ed7f-06c6-4863-a54d-2b790f2176f2";
const CAPTURE_GAME: &CStr = c"Minecraft";
const LOG_FILE: &str = "minecraft-native.log";

// The open pipeline read by the message observer on the GLFW owner thread.
static INPUT_PIPELINE: AtomicPtr<rsf_gl_pipeline> = AtomicPtr::new(null_mut());
static LOG: Mutex<Option<File>> = Mutex::new(None);

fn write_log(message: &[u8]) {
    let mut file = LOG.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(file) = file.as_mut() else {
        return;
    };
    // The C runtime text stream wrote CRLF line ends; keep the file byte-compatible.
    let mut line = Vec::with_capacity(message.len() + 2);
    for &byte in message {
        if byte == b'\n' {
            line.push(b'\r');
        }
        line.push(byte);
    }
    line.extend_from_slice(b"\r\n");
    let _ = file.write_all(&line);
    let _ = file.flush();
}

/// The Runtime and capture log. Lines written before the log opens are dropped, as before.
unsafe extern "C" fn log(_user: *mut c_void, message: *const c_char) {
    if message.is_null() {
        return;
    }
    // SAFETY: the Runtime passes a terminated message that lives for the call.
    write_log(unsafe { CStr::from_ptr(message) }.to_bytes());
}

fn open_log(root: &Path) {
    let mut file = LOG.lock().unwrap_or_else(PoisonError::into_inner);
    if file.is_none() {
        *file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(root.join(LOG_FILE))
            .ok();
    }
}

// A removed GLFW message is reported with its original identity.
unsafe extern "C" fn report_message(message: u32) {
    unsafe { rsf_gl_pipeline_input(INPUT_PIPELINE.load(SeqCst), 0, 0, message) };
}

// The JVM provides a live, thread-local JNIEnv and its JNI function table on each entry.
unsafe fn interface<'jni>(env: *mut JNIEnv) -> &'jni JNINativeInterface_ {
    unsafe { &**env }
}

fn pipeline(handle: jlong) -> *mut rsf_gl_pipeline {
    handle as usize as *mut rsf_gl_pipeline
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_createSurface")]
unsafe extern "system" fn create_surface(_: *mut JNIEnv, _: jclass, parent: jlong) -> jlong {
    let parent = parent as usize as *mut c_void;
    let surface = unsafe { rsf_gl_window_create(parent) };
    if !surface.is_null() && !observer::install(parent, surface, Some(report_message)) {
        unsafe { rsf_gl_window_destroy(surface) };
        return 0;
    }
    surface as usize as jlong
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_destroySurface")]
unsafe extern "system" fn destroy_surface(_: *mut JNIEnv, _: jclass, surface: jlong) {
    observer::close();
    unsafe { rsf_gl_window_destroy(surface as usize as *mut c_void) };
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_open")]
unsafe extern "system" fn open(
    env: *mut JNIEnv,
    _: jclass,
    hwnd: jlong,
    surface: jlong,
    root: jstring,
) -> jlong {
    if root.is_null() {
        return 0;
    }
    let api = unsafe { interface(env) };
    let length = unsafe { (api.GetStringLength.unwrap())(env, root) };
    let chars = unsafe { (api.GetStringChars.unwrap())(env, root, null_mut()) };
    if chars.is_null() {
        return 0;
    }
    // The JVM lends length UTF-16 code units until ReleaseStringChars.
    let directory =
        runtime_directory(unsafe { std::slice::from_raw_parts(chars, length as usize) });
    unsafe { (api.ReleaseStringChars.unwrap())(env, root, chars) };
    let Some((directory, utf8)) = directory else {
        return 0;
    };
    open_log(Path::new(&directory));
    let setup = rsf_gl_pipeline_setup {
        struct_size: size_of::<rsf_gl_pipeline_setup>() as u32,
        hwnd: hwnd as usize as *mut c_void,
        runtime_directory_utf8: utf8.as_ptr(),
        log: Some(log),
        user: null_mut(),
        engine_version_utf8: ENGINE_VERSION.as_ptr(),
        project_id_utf8: PROJECT_ID.as_ptr(),
        presentation_hwnd: surface as usize as *mut c_void,
    };
    unsafe {
        rsf_input_dump_configure(null(), CAPTURE_GAME.as_ptr(), null(), Some(log), null_mut())
    };
    let mut pipeline = null_mut();
    let result = unsafe { rsf_gl_pipeline_open(&setup, &mut pipeline) };
    if result != OK {
        write_log(format!("Minecraft native open failed: {result}").as_bytes());
        if !pipeline.is_null() && unsafe { rsf_gl_pipeline_close(pipeline) } == OK {
            pipeline = null_mut();
        }
    }
    INPUT_PIPELINE.store(pipeline, SeqCst);
    pipeline as usize as jlong
}

fn surfaces(pipeline: *mut rsf_gl_pipeline) -> Result<rsf_gl_pipeline_surfaces, i32> {
    let mut data = rsf_gl_pipeline_surfaces {
        struct_size: size_of::<rsf_gl_pipeline_surfaces>() as u32,
        ..Default::default()
    };
    let result = unsafe { rsf_gl_pipeline_get_surfaces(pipeline, &mut data) };
    if result != OK {
        return Err(result);
    }
    Ok(data)
}

unsafe fn configuration(
    env: *mut JNIEnv,
    [sr, quality, preset, fg, count, reflex, limit]: [jint; 7],
    debug: jboolean,
    sharpening: jfloatArray,
) -> Option<Configuration> {
    let api = unsafe { interface(env) };
    let mut configuration = Configuration {
        sr,
        quality,
        preset,
        fg,
        count,
        reflex,
        limit,
        debug: debug != 0,
        sharpening: [0.0; frame::SHARPENING_FIELDS],
    };
    let length = configuration.sharpening.len() as jint;
    if sharpening.is_null() || unsafe { (api.GetArrayLength.unwrap())(env, sharpening) } != length {
        return None;
    }
    unsafe {
        (api.GetFloatArrayRegion.unwrap())(
            env,
            sharpening,
            0,
            length,
            configuration.sharpening.as_mut_ptr(),
        );
        if (api.ExceptionCheck.unwrap())(env) != 0 {
            return None;
        }
    }
    Some(configuration)
}

unsafe fn configured_surfaces(
    env: *mut JNIEnv,
    pipeline: *mut rsf_gl_pipeline,
    width: jint,
    height: jint,
) -> jintArray {
    let api = unsafe { interface(env) };
    let Ok(current) = surfaces(pipeline) else {
        return null_mut();
    };
    let values = frame::surface_values(&current, width, height);
    let count = values.len() as jint;
    let array = unsafe { (api.NewIntArray.unwrap())(env, count) };
    if !array.is_null() {
        unsafe { (api.SetIntArrayRegion.unwrap())(env, array, 0, count, values.as_ptr()) };
    }
    array
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_configure")]
unsafe extern "system" fn configure(
    env: *mut JNIEnv,
    _: jclass,
    handle: jlong,
    width: jint,
    height: jint,
    sr: jint,
    quality: jint,
    preset: jint,
    fg: jint,
    count: jint,
    reflex: jint,
    limit: jint,
    debug: jboolean,
    sharpening: jfloatArray,
) -> jintArray {
    let Some(configuration) = (unsafe {
        configuration(
            env,
            [sr, quality, preset, fg, count, reflex, limit],
            debug,
            sharpening,
        )
    }) else {
        return null_mut();
    };
    let pipeline = pipeline(handle);
    let options = frame::options(&configuration);
    let result = unsafe {
        rsf_gl_pipeline_configure_legacy(
            pipeline,
            &options,
            configuration.limit.max(0) as u32,
            width as u32,
            height as u32,
        )
    };
    if result != OK {
        return null_mut();
    }
    unsafe { configured_surfaces(env, pipeline, width, height) }
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_openSettings")]
unsafe extern "system" fn open_settings(
    env: *mut JNIEnv,
    _: jclass,
    handle: jlong,
    path: jstring,
    sr: jint,
    quality: jint,
    preset: jint,
    fg: jint,
    count: jint,
    reflex: jint,
    limit: jint,
    debug: jboolean,
    sharpening: jfloatArray,
) -> jint {
    let Some(configuration) = (unsafe {
        configuration(
            env,
            [sr, quality, preset, fg, count, reflex, limit],
            debug,
            sharpening,
        )
    }) else {
        return -1;
    };
    if path.is_null() {
        return -1;
    }
    let api = unsafe { interface(env) };
    let length = unsafe { (api.GetStringLength.unwrap())(env, path) };
    let chars = unsafe { (api.GetStringChars.unwrap())(env, path, null_mut()) };
    if chars.is_null() {
        return -1;
    }
    let mut settings = unsafe { std::slice::from_raw_parts(chars, length as usize) }.to_vec();
    unsafe { (api.ReleaseStringChars.unwrap())(env, path, chars) };
    if settings.contains(&0) {
        return -1;
    }
    settings.push(0);
    let section: Vec<u16> = "Render".encode_utf16().chain([0]).collect();
    let options = frame::options(&configuration);
    unsafe {
        rsf_gl_pipeline_settings_open(
            pipeline(handle),
            settings.as_ptr(),
            section.as_ptr(),
            CAPTURE_GAME.as_ptr(),
            &options,
            configuration.limit.max(0) as u32,
        )
    }
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_settingsRevision")]
unsafe extern "system" fn settings_revision(_: *mut JNIEnv, _: jclass, handle: jlong) -> jlong {
    unsafe { rsf_gl_pipeline_settings_revision(pipeline(handle)) as jlong }
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_configureSettings")]
unsafe extern "system" fn configure_settings(
    env: *mut JNIEnv,
    _: jclass,
    handle: jlong,
    width: jint,
    height: jint,
) -> jintArray {
    let pipeline = pipeline(handle);
    if unsafe { rsf_gl_pipeline_configure_settings(pipeline, width as u32, height as u32) } != OK {
        return null_mut();
    }
    unsafe { configured_surfaces(env, pipeline, width, height) }
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_cpu")]
unsafe extern "system" fn cpu(_: *mut JNIEnv, _: jclass, handle: jlong, frame: jlong, stage: jint) {
    unsafe { rsf_gl_pipeline_cpu(pipeline(handle), frame as u64, stage as u32) };
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_input")]
unsafe extern "system" fn input(
    _: *mut JNIEnv,
    _: jclass,
    handle: jlong,
    frame: jlong,
    kinds: jint,
) {
    unsafe { rsf_gl_pipeline_input(pipeline(handle), frame as u64, kinds as u32, 0) };
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_abort")]
unsafe extern "system" fn abort(_: *mut JNIEnv, _: jclass, handle: jlong, frame: jlong) {
    unsafe { rsf_gl_pipeline_abort(pipeline(handle), frame as u64) };
}

unsafe fn camera_pass(
    env: *mut JNIEnv,
    pipeline: *mut rsf_gl_pipeline,
    frame: jlong,
    camera: jobject,
    reset: bool,
    screen: u32,
) -> Result<rsf_game_render_pass, i32> {
    if camera.is_null() {
        return Ok(frame::empty_pass());
    }
    let api = unsafe { interface(env) };
    let address = unsafe { (api.GetDirectBufferAddress.unwrap())(env, camera) };
    if address.is_null()
        || unsafe { (api.GetDirectBufferCapacity.unwrap())(env, camera) }
            < frame::CAMERA_FLOATS as jlong
    {
        return Ok(frame::empty_pass());
    }
    let current = surfaces(pipeline)?;
    // JNI reports FloatBuffer capacity in float elements. A direct slice may be unaligned.
    let packet = unsafe { address.cast::<frame::CameraPacket>().read_unaligned() };
    Ok(frame::pass(&packet, &current, frame as u64, reset, screen))
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_evaluate")]
unsafe extern "system" fn evaluate(
    env: *mut JNIEnv,
    _: jclass,
    handle: jlong,
    frame: jlong,
    camera: jobject,
    reset: jboolean,
    screen: jint,
) -> jint {
    let pipeline = pipeline(handle);
    match unsafe { camera_pass(env, pipeline, frame, camera, reset != 0, screen as u32) } {
        Ok(pass) => unsafe { rsf_gl_pipeline_evaluate(pipeline, &pass) },
        Err(error) => error,
    }
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_hudless")]
unsafe extern "system" fn hudless(
    env: *mut JNIEnv,
    _: jclass,
    handle: jlong,
    frame: jlong,
    camera: jobject,
) -> jint {
    let pipeline = pipeline(handle);
    match unsafe { camera_pass(env, pipeline, frame, camera, false, frame::FLIGHT_SCREEN) } {
        Ok(pass) => unsafe { rsf_gl_pipeline_hudless(pipeline, &pass) },
        Err(error) => error,
    }
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_present")]
unsafe extern "system" fn present(
    _: *mut JNIEnv,
    _: jclass,
    handle: jlong,
    frame: jlong,
    vsync: jboolean,
) -> jint {
    unsafe { rsf_gl_pipeline_present(pipeline(handle), frame as u64, u32::from(vsync)) }
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_status")]
unsafe extern "system" fn status(
    env: *mut JNIEnv,
    class: jclass,
    handle: jlong,
    destination: jlongArray,
) {
    unsafe { status_count(env, class, handle, destination) };
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_statusCount")]
unsafe extern "system" fn status_count(
    env: *mut JNIEnv,
    _: jclass,
    handle: jlong,
    destination: jlongArray,
) -> jint {
    if destination.is_null() {
        return 0;
    }
    let api = unsafe { interface(env) };
    let capacity = unsafe { (api.GetArrayLength.unwrap())(env, destination) };
    if capacity < STATUS_MINIMUM as jint {
        return 0;
    }
    let mut status = rsf_gl_pipeline_status {
        struct_size: size_of::<rsf_gl_pipeline_status>() as u32,
        ..Default::default()
    };
    if unsafe { rsf_gl_pipeline_status_get(pipeline(handle), &mut status) } != OK {
        return 0;
    }
    let mut latency = rsf_native_latency_status {
        struct_size: size_of::<rsf_native_latency_status>() as u32,
        version: 1,
        ..Default::default()
    };
    let latency_available = unsafe { rsf_native_latency_status_get(&mut latency) } != 0;
    let values = status_values(&status, latency_available.then_some(&latency));
    let count = capacity.min(values.len() as jint);
    unsafe { (api.SetLongArrayRegion.unwrap())(env, destination, 0, count, values.as_ptr()) };
    count
}

#[unsafe(export_name = "Java_org_kpc_rescaleframecraft_render_NativeBridge_close")]
unsafe extern "system" fn close(_: *mut JNIEnv, _: jclass, handle: jlong) -> jint {
    INPUT_PIPELINE.store(null_mut(), SeqCst);
    unsafe { rsf_gl_pipeline_close(pipeline(handle)) }
}
