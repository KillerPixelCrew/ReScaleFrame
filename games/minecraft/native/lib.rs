// SPDX-License-Identifier: MIT
//! Minecraft Java 1.21.1 (NeoForge) native adapter owner behind
//! org.kpc.rescaleframecraft.render.NativeBridge: the native session with its identity and log,
//! the presentation child and GLFW message observer, the camera packet, the Sodium settings
//! translation and the status array the Java renderer reads.
//!
//! jni.cpp keeps the exported JNI names and their typed marshaling and lends this crate the linked
//! OrchestratorCore through rsf_minecraft_platform (minecraft_platform.h). The Runtime decides
//! vendors, presentation, pacing and frame policy; this crate reports Minecraft's facts only.
#![cfg(windows)]

mod frame;
mod observer;
mod platform_abi;

use platform_abi::{
    RSF_MINECRAFT_HUDLESS_SCREEN, RSF_MINECRAFT_STATUS_MINIMUM, rsf_backend_result,
    rsf_gl_pipeline, rsf_gl_pipeline_options, rsf_gl_pipeline_setup, rsf_gl_pipeline_status,
    rsf_gl_pipeline_surfaces, rsf_minecraft_configuration, rsf_minecraft_message_fn,
    rsf_minecraft_platform, rsf_native_latency_status, rsf_sharpening_options,
};
use rsf_game_sys::rsf_game_render_pass;
use std::ffi::{CStr, c_char, c_void};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering::SeqCst};
use std::sync::{Mutex, OnceLock, PoisonError};

const RSF_BACKEND_OK: rsf_backend_result = 0;
// Returned only if jni.cpp ever lent an incomplete table.
const RSF_BACKEND_ERROR_INVALID_ARGUMENT: rsf_backend_result = -1;
// Adapter-supplied identity for vendor SDKs and captures.
const ENGINE_VERSION: &CStr = c"Minecraft 1.21.1";
const PROJECT_ID: &CStr = c"11e3ed7f-06c6-4863-a54d-2b790f2176f2";
const CAPTURE_GAME: &CStr = c"Minecraft";
const LOG_FILE: &str = "minecraft-native.log";

/// The validated platform table. Every entry is present.
struct Ops(rsf_minecraft_platform);

static OPS: OnceLock<Ops> = OnceLock::new();
// The open pipeline, read by the message observer on the GLFW owner thread.
static INPUT_PIPELINE: AtomicPtr<rsf_gl_pipeline> = AtomicPtr::new(null_mut());
// Opened by the first NativeBridge.open with a writable runtime directory; kept for the process.
static LOG: Mutex<Option<File>> = Mutex::new(None);

/// # Safety
/// `table` is null or points at a platform table.
unsafe fn ops(table: *const rsf_minecraft_platform) -> Option<&'static Ops> {
    if let Some(ops) = OPS.get() {
        return Some(ops);
    }
    // SAFETY: the caller passes null or a valid table.
    let table = unsafe { table.as_ref() }?;
    Ops::complete(table).then(|| OPS.get_or_init(|| Ops(*table)))
}

impl Ops {
    fn complete(table: &rsf_minecraft_platform) -> bool {
        table.window_create.is_some()
            && table.window_destroy.is_some()
            && table.open.is_some()
            && table.configure.is_some()
            && table.get_surfaces.is_some()
            && table.cpu.is_some()
            && table.input.is_some()
            && table.abort.is_some()
            && table.evaluate.is_some()
            && table.hudless.is_some()
            && table.present.is_some()
            && table.status_get.is_some()
            && table.close.is_some()
            && table.input_dump_configure.is_some()
            && table.latency_status_get.is_some()
            && table.sharpening_defaults.is_some()
            && table.sharpening_valid.is_some()
    }

    // Every entry below is a linked Runtime call that accepts any pipeline the Runtime returned,
    // including null, exactly as the C++ adapter passed them. SAFETY for each unsafe block: the
    // entry is present (complete) and its pointer arguments are live for the call.

    fn window_create(&self, parent: *mut c_void) -> *mut c_void {
        self.0
            .window_create
            .map_or(null_mut(), |f| unsafe { f(parent) })
    }

    fn window_destroy(&self, window: *mut c_void) {
        if let Some(f) = self.0.window_destroy {
            unsafe { f(window) };
        }
    }

    fn open(&self, setup: &rsf_gl_pipeline_setup, pipeline: &mut *mut rsf_gl_pipeline) -> i32 {
        self.0
            .open
            .map_or(RSF_BACKEND_ERROR_INVALID_ARGUMENT, |f| unsafe {
                f(setup, pipeline)
            })
    }

    fn configure(
        &self,
        pipeline: *mut rsf_gl_pipeline,
        options: &rsf_gl_pipeline_options,
        width: u32,
        height: u32,
    ) -> i32 {
        self.0
            .configure
            .map_or(RSF_BACKEND_ERROR_INVALID_ARGUMENT, |f| unsafe {
                f(pipeline, options, width, height)
            })
    }

    fn get_surfaces(
        &self,
        pipeline: *mut rsf_gl_pipeline,
        surfaces: &mut rsf_gl_pipeline_surfaces,
    ) -> i32 {
        self.0
            .get_surfaces
            .map_or(RSF_BACKEND_ERROR_INVALID_ARGUMENT, |f| unsafe {
                f(pipeline, surfaces)
            })
    }

    fn cpu(&self, pipeline: *mut rsf_gl_pipeline, frame: u64, stage: u32) {
        if let Some(f) = self.0.cpu {
            unsafe { f(pipeline, frame, stage) };
        }
    }

    fn input(&self, pipeline: *mut rsf_gl_pipeline, frame: u64, kinds: u32, message: u32) {
        if let Some(f) = self.0.input {
            unsafe { f(pipeline, frame, kinds, message) };
        }
    }

    fn abort(&self, pipeline: *mut rsf_gl_pipeline, frame: u64) {
        if let Some(f) = self.0.abort {
            unsafe { f(pipeline, frame) };
        }
    }

    fn evaluate(&self, pipeline: *mut rsf_gl_pipeline, pass: &rsf_game_render_pass) -> i32 {
        self.0
            .evaluate
            .map_or(RSF_BACKEND_ERROR_INVALID_ARGUMENT, |f| unsafe {
                f(pipeline, pass)
            })
    }

    fn hudless(&self, pipeline: *mut rsf_gl_pipeline, pass: &rsf_game_render_pass) -> i32 {
        self.0
            .hudless
            .map_or(RSF_BACKEND_ERROR_INVALID_ARGUMENT, |f| unsafe {
                f(pipeline, pass)
            })
    }

    fn present(&self, pipeline: *mut rsf_gl_pipeline, frame: u64, vsync: u32) -> i32 {
        self.0
            .present
            .map_or(RSF_BACKEND_ERROR_INVALID_ARGUMENT, |f| unsafe {
                f(pipeline, frame, vsync)
            })
    }

    fn status_get(&self, pipeline: *mut rsf_gl_pipeline, status: &mut rsf_gl_pipeline_status) {
        if let Some(f) = self.0.status_get {
            unsafe { f(pipeline, status) };
        }
    }

    fn close(&self, pipeline: *mut rsf_gl_pipeline) -> i32 {
        self.0
            .close
            .map_or(RSF_BACKEND_ERROR_INVALID_ARGUMENT, |f| unsafe {
                f(pipeline)
            })
    }

    fn configure_captures(&self) {
        if let Some(f) = self.0.input_dump_configure {
            unsafe { f(null(), CAPTURE_GAME.as_ptr(), null(), Some(log), null_mut()) };
        }
    }

    fn latency_status(&self, status: &mut rsf_native_latency_status) {
        if let Some(f) = self.0.latency_status_get {
            unsafe { f(status) };
        }
    }

    fn sharpening_defaults(&self) -> rsf_sharpening_options {
        let mut options = rsf_sharpening_options::default();
        if let Some(f) = self.0.sharpening_defaults {
            unsafe { f(&mut options) };
        }
        options
    }

    fn sharpening_valid(&self, options: &rsf_sharpening_options) -> bool {
        self.0
            .sharpening_valid
            .is_some_and(|f| unsafe { f(options) })
    }
}

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

fn open_log(root: &CStr) {
    let mut file = LOG.lock().unwrap_or_else(PoisonError::into_inner);
    if file.is_none() {
        let root = String::from_utf8_lossy(root.to_bytes());
        *file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(Path::new(root.as_ref()).join(LOG_FILE))
            .ok();
    }
}

// The observer sink: a removed message goes to the open pipeline's input consumer.
unsafe extern "C" fn report_message(message: u32) {
    if let Some(ops) = OPS.get() {
        ops.input(INPUT_PIPELINE.load(SeqCst), 0, 0, message);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_minecraft_create_surface(
    platform: *const rsf_minecraft_platform,
    parent: *mut c_void,
) -> *mut c_void {
    // SAFETY: jni.cpp passes its static table.
    let Some(ops) = (unsafe { ops(platform) }) else {
        return null_mut();
    };
    let surface = ops.window_create(parent);
    if !surface.is_null() && !observer::install(parent, surface, Some(report_message)) {
        ops.window_destroy(surface);
        return null_mut();
    }
    surface
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_minecraft_destroy_surface(
    platform: *const rsf_minecraft_platform,
    window: *mut c_void,
) {
    observer::close();
    // SAFETY: jni.cpp passes its static table.
    if let Some(ops) = unsafe { ops(platform) } {
        ops.window_destroy(window);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_minecraft_open(
    platform: *const rsf_minecraft_platform,
    hwnd: *mut c_void,
    surface: *mut c_void,
    root: *const c_char,
) -> *mut rsf_gl_pipeline {
    // SAFETY: jni.cpp passes its static table.
    let Some(ops) = (unsafe { ops(platform) }) else {
        return null_mut();
    };
    if root.is_null() {
        return null_mut();
    }
    // SAFETY: root is the terminated JNI UTF chars, released after this call.
    open_log(unsafe { CStr::from_ptr(root) });
    let setup = rsf_gl_pipeline_setup {
        struct_size: size_of::<rsf_gl_pipeline_setup>() as u32,
        hwnd,
        runtime_directory_utf8: root,
        log: Some(log),
        user: null_mut(),
        engine_version_utf8: ENGINE_VERSION.as_ptr(),
        project_id_utf8: PROJECT_ID.as_ptr(),
        presentation_hwnd: surface,
    };
    ops.configure_captures();
    let mut pipeline = null_mut();
    let result = ops.open(&setup, &mut pipeline);
    if result != RSF_BACKEND_OK {
        write_log(format!("Minecraft native open failed: {result}").as_bytes());
    }
    INPUT_PIPELINE.store(pipeline, SeqCst);
    pipeline
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_minecraft_configure(
    platform: *const rsf_minecraft_platform,
    pipeline: *mut rsf_gl_pipeline,
    configuration: *const rsf_minecraft_configuration,
    surfaces: *mut i32,
) -> bool {
    // SAFETY: jni.cpp passes its static table, a live configuration and a ten-value array.
    let (Some(ops), Some(configuration)) =
        (unsafe { ops(platform) }, unsafe { configuration.as_ref() })
    else {
        return false;
    };
    if surfaces.is_null() {
        return false;
    }
    let options = frame::options(configuration, ops.sharpening_defaults());
    if !ops.sharpening_valid(&options.sharpening) {
        return false;
    }
    let (width, height) = (configuration.width, configuration.height);
    let result = ops.configure(pipeline, &options, width as u32, height as u32);
    let mut current = rsf_gl_pipeline_surfaces {
        struct_size: size_of::<rsf_gl_pipeline_surfaces>() as u32,
        ..Default::default()
    };
    if result != RSF_BACKEND_OK || ops.get_surfaces(pipeline, &mut current) != RSF_BACKEND_OK {
        return false;
    }
    let values = frame::surface_values(&current, width, height);
    // SAFETY: surfaces holds RSF_MINECRAFT_SURFACE_FIELDS values.
    unsafe { surfaces.copy_from_nonoverlapping(values.as_ptr(), values.len()) };
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_minecraft_cpu(
    platform: *const rsf_minecraft_platform,
    pipeline: *mut rsf_gl_pipeline,
    frame: u64,
    stage: u32,
) {
    // SAFETY: jni.cpp passes its static table.
    if let Some(ops) = unsafe { ops(platform) } {
        ops.cpu(pipeline, frame, stage);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_minecraft_input(
    platform: *const rsf_minecraft_platform,
    pipeline: *mut rsf_gl_pipeline,
    frame: u64,
    kinds: u32,
) {
    // SAFETY: jni.cpp passes its static table.
    if let Some(ops) = unsafe { ops(platform) } {
        ops.input(pipeline, frame, kinds, 0);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_minecraft_abort(
    platform: *const rsf_minecraft_platform,
    pipeline: *mut rsf_gl_pipeline,
    frame: u64,
) {
    // SAFETY: jni.cpp passes its static table.
    if let Some(ops) = unsafe { ops(platform) } {
        ops.abort(pipeline, frame);
    }
}

/// The SR pass for a direct camera buffer. A missing or short buffer yields the empty pass and
/// skips the surface query, as before.
///
/// # Safety
/// `camera` is null or addresses `floats` readable values.
unsafe fn camera_pass(
    ops: &Ops,
    pipeline: *mut rsf_gl_pipeline,
    frame: u64,
    camera: *const f32,
    floats: i64,
    reset: bool,
    screen: u32,
) -> rsf_game_render_pass {
    if camera.is_null() || floats < frame::CAMERA_FLOATS as i64 {
        return frame::empty_pass();
    }
    // SAFETY: the direct FloatBuffer holds at least CAMERA_FLOATS values for the call.
    let packet = unsafe { &*camera.cast::<frame::CameraPacket>() };
    let mut surfaces = rsf_gl_pipeline_surfaces {
        struct_size: size_of::<rsf_gl_pipeline_surfaces>() as u32,
        ..Default::default()
    };
    ops.get_surfaces(pipeline, &mut surfaces);
    frame::pass(packet, &surfaces, frame, reset, screen)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_minecraft_evaluate(
    platform: *const rsf_minecraft_platform,
    pipeline: *mut rsf_gl_pipeline,
    frame: u64,
    camera: *const f32,
    floats: i64,
    reset: u32,
    screen: i32,
) -> i32 {
    // SAFETY: jni.cpp passes its static table.
    let Some(ops) = (unsafe { ops(platform) }) else {
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    };
    // SAFETY: jni.cpp passes the buffer's address and capacity.
    let pass = unsafe {
        camera_pass(
            ops,
            pipeline,
            frame,
            camera,
            floats,
            reset != 0,
            screen as u32,
        )
    };
    ops.evaluate(pipeline, &pass)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_minecraft_hudless(
    platform: *const rsf_minecraft_platform,
    pipeline: *mut rsf_gl_pipeline,
    frame: u64,
    camera: *const f32,
    floats: i64,
) -> i32 {
    // SAFETY: jni.cpp passes its static table.
    let Some(ops) = (unsafe { ops(platform) }) else {
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    };
    // SAFETY: jni.cpp passes the buffer's address and capacity.
    let pass = unsafe {
        camera_pass(
            ops,
            pipeline,
            frame,
            camera,
            floats,
            false,
            RSF_MINECRAFT_HUDLESS_SCREEN,
        )
    };
    ops.hudless(pipeline, &pass)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_minecraft_present(
    platform: *const rsf_minecraft_platform,
    pipeline: *mut rsf_gl_pipeline,
    frame: u64,
    vsync: u32,
) -> i32 {
    // SAFETY: jni.cpp passes its static table.
    unsafe { ops(platform) }.map_or(RSF_BACKEND_ERROR_INVALID_ARGUMENT, |ops| {
        ops.present(pipeline, frame, vsync)
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_minecraft_status(
    platform: *const rsf_minecraft_platform,
    pipeline: *mut rsf_gl_pipeline,
    capacity: i64,
    values: *mut i64,
) -> u32 {
    if values.is_null() || capacity < i64::from(RSF_MINECRAFT_STATUS_MINIMUM) {
        return 0;
    }
    // SAFETY: jni.cpp passes its static table.
    let Some(ops) = (unsafe { ops(platform) }) else {
        return 0;
    };
    let mut status = rsf_gl_pipeline_status {
        struct_size: size_of::<rsf_gl_pipeline_status>() as u32,
        ..Default::default()
    };
    ops.status_get(pipeline, &mut status);
    let mut latency = rsf_native_latency_status {
        struct_size: size_of::<rsf_native_latency_status>() as u32,
        version: 1,
        ..Default::default()
    };
    ops.latency_status(&mut latency);
    let packed = frame::status_values(&status, &latency);
    // SAFETY: values holds RSF_MINECRAFT_STATUS_FIELDS values.
    unsafe { values.copy_from_nonoverlapping(packed.as_ptr(), packed.len()) };
    capacity.min(packed.len() as i64) as u32
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_minecraft_close(
    platform: *const rsf_minecraft_platform,
    pipeline: *mut rsf_gl_pipeline,
) -> i32 {
    INPUT_PIPELINE.store(null_mut(), SeqCst);
    // SAFETY: jni.cpp passes its static table.
    unsafe { ops(platform) }.map_or(RSF_BACKEND_ERROR_INVALID_ARGUMENT, |ops| {
        ops.close(pipeline)
    })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_minecraft_observer_install(
    parent: *mut c_void,
    child: *mut c_void,
    sink: rsf_minecraft_message_fn,
) -> bool {
    observer::install(parent, child, sink)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_minecraft_observer_close() {
    observer::close();
}
