// SPDX-License-Identifier: MIT
//! Opt-in hardware scenarios for the GL pipeline and the Minecraft event observer.
use super::{
    FixtureError,
    gl_fixture::{Framebuffer, Window},
    log, require,
};
use crate::gl_pipeline_fixture_abi::*;
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_RENDER_RESET, RSF_SCREEN_FLIGHT, rsf_camera_frame,
    rsf_game_render_pass,
};
use std::ffi::{CStr, CString, c_void};
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::thread;
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    Graphics::{Gdi::HDC, OpenGL::*},
    Storage::FileSystem::GetTempFileNameW,
    System::{Threading::GetCurrentThreadId, WindowsProgramming::GetPrivateProfileIntW},
    UI::WindowsAndMessaging::*,
};
use windows::core::{PCWSTR, c, w};

const OK: i32 = 0;
const INVALID: i32 = -1;
const SCENARIOS: [(u32, u32, u32, u32); 13] = [
    (0, 0, 1280, 720),
    (2, 0, 1280, 720),
    (3, 0, 1280, 720),
    (5, 0, 1280, 720),
    (1, 0, 1280, 720),
    (0, 3, 1280, 720),
    (3, 5, 1280, 720),
    (1, 1, 1280, 720),
    (1, 1, 854, 480),
    (1, 1, 1920, 1080),
    (1, 1, 2560, 1494),
    (1, 1, 1280, 720),
    (0, 0, 1280, 720),
];

static PROBE: AtomicU32 = AtomicU32::new(0);
static DEQUEUED: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" {
    fn rsf_minecraft_observer_install(
        parent: *mut c_void,
        child: *mut c_void,
        sink: Option<unsafe extern "C" fn(u32)>,
    ) -> bool;
    fn rsf_minecraft_observer_close();
}

unsafe extern "C" fn observe(message: u32) {
    if message == PROBE.load(Relaxed) {
        DEQUEUED.fetch_add(1, Relaxed);
    }
}

struct Child(HWND);
impl Drop for Child {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe { rsf_gl_window_destroy(self.0.0) };
        }
    }
}

struct Observer;
impl Drop for Observer {
    fn drop(&mut self) {
        unsafe { rsf_minecraft_observer_close() };
    }
}

fn verify_input(parent: HWND, child: HWND) -> Result<(), FixtureError> {
    unsafe {
        let probe = RegisterWindowMessageW(w!("ReScaleFrame.HiddenInputProbe"));
        require(probe != 0, "register input probe")?;
        PROBE.store(probe, Relaxed);
        DEQUEUED.store(0, Relaxed);
        PostMessageW(Some(parent), probe, WPARAM(0), LPARAM(0))
            .map_err(|_| FixtureError::Fail("post window input probe"))?;
        let mut msg = MSG::default();
        require(
            PeekMessageW(&mut msg, Some(parent), probe, probe, PM_NOREMOVE).as_bool()
                && DEQUEUED.load(Relaxed) == 0,
            "peek without consuming does not record input",
        )?;
        require(
            PeekMessageW(&mut msg, Some(parent), probe, probe, PM_REMOVE).as_bool()
                && DEQUEUED.load(Relaxed) == 1,
            "dequeued window ping reaches one simulation",
        )?;
        PostThreadMessageW(GetCurrentThreadId(), probe, WPARAM(0), LPARAM(0))
            .map_err(|_| FixtureError::Fail("post thread input probe"))?;
        require(
            PeekMessageW(&mut msg, None, probe, probe, PM_REMOVE).as_bool()
                && DEQUEUED.load(Relaxed) == 2,
            "dequeued thread ping shares input boundary",
        )?;
        require(
            SendMessageW(child, WM_NCHITTEST, None, None).0 == HTTRANSPARENT as isize,
            "DXGI child passes hit tests to the parent",
        )
    }
}

pub(super) fn run(window: &Window) -> Result<(), FixtureError> {
    let Some(runtime) = std::env::var_os("RSF_GL_TEST_RUNTIME") else {
        return Ok(());
    };
    let runtime = CString::new(runtime.to_string_lossy().as_bytes())
        .map_err(|_| FixtureError::Fail("invalid vendor runtime directory"))?;
    unsafe {
        SwapBuffers(window.dc)
            .map_err(|_| FixtureError::Fail("present WGL before DXGI adoption"))?;
    }
    let mut child = Child(HWND(unsafe { rsf_gl_window_create(window.hwnd.0) }));
    require(!child.0.0.is_null(), "create child on event thread")?;
    require(
        unsafe { rsf_minecraft_observer_install(window.hwnd.0, child.0.0, Some(observe)) },
        "observe event owner's dequeued input",
    )?;
    let observer = Observer;
    verify_input(window.hwnd, child.0)?;
    unsafe { wglMakeCurrent(HDC::default(), HGLRC::default()) }
        .map_err(|_| FixtureError::Fail("release context on event thread"))?;
    // The event thread keeps the HWND and context alive until the render thread has joined.
    let (parent, child_handle, dc, context) = (
        window.hwnd.0 as usize,
        child.0.0 as usize,
        window.dc.0 as usize,
        window.context.0 as usize,
    );
    let graphics = thread::spawn(move || {
        unsafe { wglMakeCurrent(HDC(dc as _), HGLRC(context as _)) }
            .map_err(|_| FixtureError::Fail("adopt context on render thread"))?;
        let result = render(parent as _, child_handle as _, &runtime);
        let released = unsafe { wglMakeCurrent(HDC::default(), HGLRC::default()) };
        result?;
        released.map_err(|_| FixtureError::Fail("release context on render thread"))
    });
    unsafe {
        while !graphics.is_finished() {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            MsgWaitForMultipleObjects(None, false, 1, QS_ALLINPUT);
        }
    }
    let result = graphics
        .join()
        .map_err(|_| FixtureError::Fail("render thread panicked"))?;
    drop(observer);
    require(
        unsafe { rsf_gl_window_destroy(child.0.0) } != 0,
        "destroy the DXGI child",
    )?;
    child.0 = HWND::default();
    result?;
    println!(
        "PASS: separate event/render threads, window ownership, transparent input hit testing, resize and shutdown."
    );
    Ok(())
}

struct Pipeline(*mut rsf_gl_pipeline);
impl Pipeline {
    fn open(parent: *mut c_void, child: *mut c_void, runtime: &CStr) -> Result<Self, FixtureError> {
        let setup = rsf_gl_pipeline_setup {
            struct_size: size_of::<rsf_gl_pipeline_setup>() as u32,
            hwnd: parent,
            runtime_directory_utf8: runtime.as_ptr(),
            log: Some(log),
            user: null_mut(),
            engine_version_utf8: c!("Minecraft 1.21.1").as_ptr(),
            project_id_utf8: c!("11e3ed7f-06c6-4863-a54d-2b790f2176f2").as_ptr(),
            presentation_hwnd: child,
        };
        let mut pipeline = Self(null_mut());
        let result = unsafe { rsf_gl_pipeline_open(&setup, &mut pipeline.0) };
        if result != OK {
            pipeline.close()?;
        }
        require(
            result == OK && !pipeline.0.is_null(),
            "open explicit DXGI presentation pipeline",
        )?;
        Ok(pipeline)
    }

    fn close(&mut self) -> Result<(), FixtureError> {
        if !self.0.is_null() {
            require(
                unsafe { rsf_gl_pipeline_close(self.0) } == OK,
                "retire GL pipeline owner",
            )?;
            self.0 = null_mut();
        }
        Ok(())
    }

    fn surfaces(&self) -> Result<rsf_gl_pipeline_surfaces, FixtureError> {
        let mut surfaces = rsf_gl_pipeline_surfaces {
            struct_size: size_of::<rsf_gl_pipeline_surfaces>() as u32,
            ..Default::default()
        };
        require(
            unsafe { rsf_gl_pipeline_get_surfaces(self.0, &mut surfaces) } == OK,
            "get published pipeline surfaces",
        )?;
        Ok(surfaces)
    }

    fn status(&self) -> Result<rsf_gl_pipeline_status, FixtureError> {
        let mut status = rsf_gl_pipeline_status {
            struct_size: size_of::<rsf_gl_pipeline_status>() as u32,
            ..Default::default()
        };
        require(
            unsafe { rsf_gl_pipeline_status_get(self.0, &mut status) } == OK,
            "get pipeline status",
        )?;
        Ok(status)
    }

    fn cpu(&self, frame: u64, stages: impl IntoIterator<Item = u32>) {
        for stage in stages {
            unsafe { rsf_gl_pipeline_cpu(self.0, frame, stage) };
        }
    }

    fn present(&self, frame: u64, label: &'static str) -> Result<(), FixtureError> {
        require(
            unsafe { rsf_gl_pipeline_present(self.0, frame, 0) } == OK,
            label,
        )
    }

    fn verify_menus(&self, frame: &mut u64) -> Result<(), FixtureError> {
        unsafe {
            require(
                rsf_gl_pipeline_present(self.0, 0, 0) == INVALID,
                "menu needs an input frame ID",
            )?;
            for _ in 0..32 {
                *frame += 1;
                self.cpu(*frame, 0..3);
                require(
                    rsf_gl_pipeline_input(self.0, *frame, 7, 0) == OK,
                    "sample all input kinds",
                )?;
                require(
                    rsf_gl_pipeline_input(self.0, *frame, 4, 0) == OK,
                    "repeat controller poll",
                )?;
                let ping = rsf_gl_pipeline_pcl_message(self.0);
                if ping != 0 {
                    require(
                        rsf_gl_pipeline_input(self.0, *frame, 0, ping) == OK,
                        "SDK ping uses the consuming input frame",
                    )?;
                }
                self.cpu(*frame, [3]);
                verify_token(*frame, true)?;
                self.present(*frame, "menu present closes its CPU frame")?;
                verify_token(*frame, false)?;
                require(
                    rsf_gl_pipeline_input(self.0, *frame, 1, 0) != OK,
                    "post-Present input cannot reuse SDK token",
                )?;
            }
            *frame += 1;
            self.cpu(*frame, 0..4);
            require(
                rsf_gl_pipeline_abort(self.0, *frame) == OK,
                "abort incomplete resized frame",
            )?;
            verify_token(*frame, false)
        }
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

unsafe fn verify_token(frame: u64, open: bool) -> Result<(), FixtureError> {
    let host = unsafe { rsf_d3d11_present_host() };
    if !host.is_null() {
        require(
            unsafe { rsf_streamline_host_token(host, frame) }.is_null() != open,
            "Streamline token lifetime matches CPU frame",
        )?;
    }
    Ok(())
}

fn frame_pass(
    surfaces: &rsf_gl_pipeline_surfaces,
    width: u32,
    height: u32,
) -> rsf_game_render_pass {
    // The zeroed C record has no active resources or depth mappings.
    let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    let mut identity = [0.0; 16];
    for i in (0..16).step_by(5) {
        identity[i] = 1.0;
    }
    pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
    pass.history_key = 1;
    pass.view_key = 1;
    pass.camera_valid = 1;
    pass.screen = RSF_SCREEN_FLIGHT;
    pass.camera.struct_size = size_of::<rsf_camera_frame>() as u32;
    pass.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    pass.camera.view_to_clip = identity;
    pass.camera.clip_to_view = identity;
    pass.camera.view_to_world = identity;
    pass.camera.world_to_view = identity;
    pass.camera.clip_to_previous_clip = identity;
    pass.camera.render_width = surfaces.render_width;
    pass.camera.render_height = surfaces.render_height;
    pass.camera.output_width = width;
    pass.camera.output_height = height;
    pass.camera.near_plane = 0.05;
    pass.camera.far_plane = 1024.0;
    pass.camera.vertical_fov_radians = 1.0;
    pass.camera.frame_time_seconds = 0.016;
    pass.motion_to_uv = [
        1.0 / surfaces.render_width as f32,
        1.0 / surfaces.render_height as f32,
    ];
    pass
}

fn options(backend: u32, generation: u32) -> rsf_gl_pipeline_options {
    rsf_gl_pipeline_options {
        struct_size: size_of::<rsf_gl_pipeline_options>() as u32,
        sr_backend: backend,
        quality: 1,
        fg_backend: generation,
        generation: rsf_native_fg_options {
            struct_size: size_of::<rsf_native_fg_options>() as u32,
            mode: if generation == 0 {
                RSF_FG_OFF
            } else {
                RSF_FG_FIXED
            },
            generated_frames: 1,
            reflex_mode: RSF_REFLEX_ON,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn clear_surface() {
    unsafe {
        glClearColor(0.25, 0.5, 0.75, 1.0);
        glClear(GL_COLOR_BUFFER_BIT);
    }
}

fn verify_colors(
    fbo: &Framebuffer,
    texture: u32,
    width: u32,
    height: u32,
) -> Result<(), FixtureError> {
    fbo.attach_texture(texture)?;
    let (right, top) = (width as i32 - 1, height as i32 - 1);
    for (x, y) in [
        (0, 0),
        (right, 0),
        (0, top),
        (right, top),
        (right / 2, top / 2),
    ] {
        let mut pixel = [0.0_f32; 4];
        unsafe { glReadPixels(x, y, 1, 1, GL_RGBA, GL_FLOAT, pixel.as_mut_ptr().cast()) };
        require(
            pixel[..3]
                .iter()
                .zip([0.25, 0.5, 0.75])
                .all(|(p, expected)| (*p - expected).abs() < 0.03),
            "vendor output preserves SDR colour at corners and centre",
        )?;
    }
    Ok(())
}

fn verify_scenarios(
    pipeline: &Pipeline,
    held: &Framebuffer,
    frame: &mut u64,
    capture: &mut Capture,
) -> Result<(), FixtureError> {
    for (backend, generation, width, height) in SCENARIOS {
        let options = options(backend, generation);
        unsafe {
            glEnable(0xdeadbeef); // An inherited GL error must not fail a new texture import.
            require(
                rsf_gl_pipeline_configure(pipeline.0, &options, width, height) == OK,
                "configure vendor and output extent",
            )?;
        }
        let surfaces = pipeline.surfaces()?;
        require(
            pipeline.status()?.effective_sr == backend,
            "requested SR backend is active",
        )?;
        let fbo = Framebuffer::new()?;
        for surface in [
            surfaces.scene,
            surfaces.depth,
            surfaces.motion,
            surfaces.hudless,
            surfaces.composed,
        ] {
            fbo.attach_texture(surface)?;
            unsafe { glViewport(0, 0, width as i32, height as i32) };
            clear_surface();
        }
        pipeline.verify_menus(frame)?;
        let mut pass = frame_pass(&surfaces, width, height);
        for iteration in 0..if generation == 0 { 3 } else { 16 } {
            if backend == RSF_SR_FSR3 && iteration == 0 {
                capture.request();
            }
            *frame += 1;
            pass.native_frame = *frame;
            pass.source_frame_id = *frame;
            pass.flags = if iteration == 0 {
                RSF_GAME_RENDER_RESET
            } else {
                0
            };
            pipeline.cpu(*frame, 0..4);
            unsafe {
                require(
                    rsf_gl_pipeline_evaluate(pipeline.0, &pass) == OK,
                    "vendor SR evaluation",
                )?;
                require(
                    rsf_gl_pipeline_hudless(pipeline.0, &pass) == OK,
                    "capture HUD-less surface",
                )?;
            }
            pipeline.present(*frame, "present composed GL image through DXGI")?;
        }
        if backend != 0 {
            verify_colors(&fbo, surfaces.output, width, height)?;
        }
        let status = pipeline.status()?;
        if generation != 0 {
            let fg = status.generation;
            println!(
                "FG backend={generation} effective={} switch={} result={} reason={} tags={} generated={}",
                status.effective_fg,
                status.fg_switch_result,
                fg.last_result,
                fg.reason,
                fg.tagged_frames,
                fg.vendor.effective_generated_frames
            );
            require(
                status.effective_fg == generation
                    && fg.last_result == OK
                    && fg.reason == 0
                    && fg.tagged_frames > 0,
                "FG provider prepared valid input/simulation/world/HUD-less frames",
            )?;
        }
        held.attach_texture(surfaces.composed)?;
        if backend == RSF_SR_DLSS {
            require(
                unsafe { rsf_gl_pipeline_configure(pipeline.0, &options, 32768, 64) } != OK,
                "reject unsupported allocation size",
            )?;
            let retained = pipeline.surfaces()?;
            require(
                retained.output == surfaces.output
                    && retained.composed == surfaces.composed
                    && unsafe { glIsTexture(retained.composed) } != 0,
                "refused resize retains published textures and identity",
            )?;
        }
        println!(
            "PASS: native SR backend {backend}, plan {}x{} -> {width}x{height}, runtime switch and DXGI presentation",
            surfaces.render_width, surfaces.render_height
        );
    }
    Ok(())
}

fn verify_latency() -> Result<(), FixtureError> {
    let mut status = rsf_native_latency_status {
        struct_size: size_of::<rsf_native_latency_status>() as u32,
        version: 1,
        ..Default::default()
    };
    require(
        unsafe { rsf_native_latency_status_get(&mut status) } != 0
            && status.events.order_violations == 0
            && status.events.substitutes == 0,
        "measured GL frames preserve latency ordering without substitutes",
    )?;
    println!(
        "GL core latency: owner={} paced={} input={:?} pings={} order={} substitutes={}",
        status.owner,
        status.paced_frames,
        status.events.input_events,
        status.events.pings,
        status.events.order_violations,
        status.events.substitutes
    );
    Ok(())
}

struct SettingsFile {
    path: PathBuf,
    wide: [u16; 260],
}

impl SettingsFile {
    fn missing() -> Result<Self, FixtureError> {
        let directory: Vec<_> = std::env::temp_dir()
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect();
        let mut wide = [0; 260];
        require(
            unsafe { GetTempFileNameW(PCWSTR(directory.as_ptr()), w!("rsf"), 0, &mut wide) } != 0,
            "reserve instance configuration filename",
        )?;
        let len = wide
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(wide.len());
        let path = PathBuf::from(
            <std::ffi::OsString as std::os::windows::ffi::OsStringExt>::from_wide(&wide[..len]),
        );
        fs::remove_file(&path)
            .map_err(|_| FixtureError::Fail("start with missing instance configuration"))?;
        Ok(Self { path, wide })
    }

    fn read(&self, key: PCWSTR) -> i32 {
        unsafe { GetPrivateProfileIntW(w!("Render"), key, 0, PCWSTR(self.wide.as_ptr())) }
    }
}

impl Drop for SettingsFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn verify_settings(
    pipeline: &Pipeline,
    file: &SettingsFile,
    parent: HWND,
    held: &Framebuffer,
    frame: &mut u64,
) -> Result<(), FixtureError> {
    let mut legacy = options(0, 0);
    legacy.generation.reflex_mode = RSF_REFLEX_OFF;
    legacy.generation.debug = 1;
    unsafe {
        require(
            rsf_gl_pipeline_settings_open(
                pipeline.0,
                file.wide.as_ptr(),
                w!("Render").0,
                c!("OpenGL fixture").as_ptr(),
                &legacy,
                60,
            ) == OK,
            "bind saved OpenGL settings",
        )?;
        require(
            file.read(w!("FrameLimitUs")) == 16666 && file.read(w!("FGDebug")) == 1,
            "migrate the frame limit and FG debug setting",
        )?;
        let mut api = rsf_overlay_api {
            struct_size: size_of::<rsf_overlay_api>() as u32,
            abi_version: RSF_OVERLAY_API_VERSION,
            ..Default::default()
        };
        require(
            rsf_get_overlay_api(&mut api) == OK,
            "get shared overlay API",
        )?;
        let get = api
            .settings_get
            .ok_or(FixtureError::Fail("missing settings getter"))?;
        let apply = api
            .settings_apply
            .ok_or(FixtureError::Fail("missing settings apply"))?;
        let mut requested = rsf_render_settings {
            struct_size: size_of::<rsf_render_settings>() as u32,
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            ..Default::default()
        };
        require(get(&mut requested) == OK, "read overlay settings")?;
        let revision = rsf_gl_pipeline_settings_revision(pipeline.0);
        requested.sr_enabled = 1;
        requested.sr_backend = RSF_SR_FSR3;
        require(
            apply(&requested) == OK && rsf_gl_pipeline_settings_revision(pipeline.0) > revision,
            "saved overlay change invalidates the configuration",
        )?;
        require(
            rsf_gl_pipeline_configure_settings(pipeline.0, 1280, 720) == OK,
            "apply shared settings at unchanged extent",
        )?;
        let status = pipeline.status()?;
        require(
            status.requested_sr == RSF_SR_FSR3 && status.effective_sr == RSF_SR_FSR3,
            "apply shared settings before simulation",
        )?;
        require(
            file.read(w!("FGDebug")) == 1,
            "settings writes preserve FG debug",
        )?;
        if std::env::var_os("RSF_GL_TEST_OVERLAY").is_some() {
            let previous = GetWindowLongPtrW(parent, GWLP_WNDPROC);
            held.attach_texture(pipeline.surfaces()?.composed)?;
            clear_surface();
            held.unbind();
            *frame += 1;
            pipeline.cpu(*frame, [0]);
            pipeline.present(*frame, "compose overlay before DXGI Present")?;
            let mut diagnostics = rsf_overlay_diagnostics {
                struct_size: size_of::<rsf_overlay_diagnostics>() as u32,
                ..Default::default()
            };
            require(
                rsf_get_overlay_diagnostics(&mut diagnostics) == OK
                    && diagnostics.presents_drawn == 1
                    && GetWindowLongPtrW(parent, GWLP_WNDPROC) != previous,
                "draw the panel and install input on the parent HWND",
            )?;
            require(
                rsf_gl_pipeline_configure_settings(pipeline.0, 854, 480) == OK,
                "retire wrapped buffers before DXGI resize",
            )?;
        }
    }
    Ok(())
}

struct Capture {
    directory: Option<PathBuf>,
    requested: bool,
}

impl Capture {
    fn configure() -> Result<Self, FixtureError> {
        let directory = std::env::var_os("RSF_GL_CAPTURE_DIR").map(PathBuf::from);
        if let Some(path) = directory.as_ref() {
            let encoded = CString::new(path.to_string_lossy().as_bytes())
                .map_err(|_| FixtureError::Fail("invalid capture path"))?;
            unsafe {
                rsf_input_dump_configure(
                    encoded.as_ptr(),
                    c!("OpenGLFixture").as_ptr(),
                    c!("fixture").as_ptr(),
                    Some(log),
                    null_mut(),
                )
            };
        }
        Ok(Self {
            directory,
            requested: false,
        })
    }

    fn request(&mut self) {
        if self.directory.is_some() && !self.requested {
            unsafe { rsf_input_dump_request() };
            self.requested = true;
        }
    }

    fn verify(&self) -> Result<(), FixtureError> {
        if let Some(directory) = self.directory.as_ref().filter(|_| self.requested) {
            let bundle = fs::read_dir(directory.join("website"))
                .map_err(|_| FixtureError::Fail("read capture directory"))?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .last()
                .ok_or(FixtureError::Fail("find frame bundle"))?;
            fn contains(bundle: &Path, name: &str, value: &str) -> bool {
                fs::read_to_string(bundle.join(name)).is_ok_and(|text| text.contains(value))
            }
            require(
                contains(&bundle, "manifest.json", "\"status\":\"complete\""),
                "frame bundle retired all GPU readbacks",
            )?;
            for name in [
                "sr_source_jittered.bin",
                "hudless_color.bin",
                "final_color.bin",
            ] {
                require(
                    fs::metadata(bundle.join(name)).is_ok_and(|file| file.len() > 0),
                    "capture includes source, HUD-less and final readbacks",
                )?;
            }
            require(
                contains(&bundle, "sr_parameters.json", "\"input_sdr\":1"),
                "capture preserves the declared SDR input",
            )?;
            println!("PASS: OpenGL core frame dump: {}", bundle.display());
        }
        Ok(())
    }
}

fn render(parent: *mut c_void, child: *mut c_void, runtime: &CStr) -> Result<(), FixtureError> {
    require(
        unsafe { rsf_gl_window_create(parent) }.is_null(),
        "refuse child creation from the render thread",
    )?;
    let mut pipeline = Pipeline::open(parent, child, runtime)?;
    let mut capture = Capture::configure()?;
    let mut frame = 0;
    let held = Framebuffer::new()?;
    verify_scenarios(&pipeline, &held, &mut frame, &mut capture)?;
    verify_latency()?;
    let settings = SettingsFile::missing()?;
    verify_settings(&pipeline, &settings, HWND(parent), &held, &mut frame)?;
    drop(held);
    pipeline.close()?;
    fs::remove_file(&settings.path).map_err(|_| FixtureError::Fail("remove fixture settings"))?;
    capture.verify()
}
