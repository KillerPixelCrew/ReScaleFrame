// SPDX-License-Identifier: MIT
//! Startup, cold COM contracts and cleanup for the linked AC7 presentation fixture.
use crate::d3d11_bridge_sr::rsf_d3d11_fixture_shared_sr;
use crate::d3d11_bridge_sr_abi::{
    rsf_dlss_frame, rsf_dlss_release_resources, rsf_dlss_share_host, rsf_dlss_shutdown,
};
use crate::d3d11_device_checks::*;
use crate::d3d11_marker::{rsf_ac7_fixture_log, rsf_d3d11_marker_fixture};
use crate::fixture_device::{rsf_d3d11_fixture_proxy_smoke, rsf_present_fixture_debug12};
use crate::present_fixture_loops_abi::*;
use crate::present_fixture_startup::*;
use rsf_game_sys::d3d11_chain_abi::{RSF_D3D11_DISPATCH_RELEASE, rsf_d3d11_chain_dispatch};
use rsf_game_sys::*;
use std::ffi::{CStr, c_char};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::Foundation::{HINSTANCE, HWND, RECT};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::Environment::GetEnvironmentVariableA;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_SYSTEM_AWARE, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, IUnknown, Interface, PCSTR, s, w};

const NOT_READY: i32 = -8;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;

static RENDERER_STARTUP_OBSERVED: AtomicBool = AtomicBool::new(false);

fn observer_status() -> (i32, rsf_observer_status) {
    let mut status = rsf_observer_status {
        struct_size: size_of::<rsf_observer_status>() as u32,
        ..Default::default()
    };
    let result = unsafe { rsf_observer_get_status(&mut status) };
    (result, status)
}

unsafe extern "C" fn before_present(_: Raw, chain: Raw) {
    let (result, status) = observer_status();
    if result != 0 || status.present_width == 0 || status.present_height == 0 {
        return;
    }
    let (mut device, mut context) = (null_mut(), null_mut());
    let acquired = unsafe { rsf_observer_acquire_device(&mut device, &mut context) };
    // Adopt even a partial observer result so every returned reference is released.
    let _device = (!device.is_null()).then(|| unsafe { IUnknown::from_raw(device) });
    let _context = (!context.is_null()).then(|| unsafe { IUnknown::from_raw(context) });
    let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }.unwrap();
    if acquired == 0
        && unsafe { chain.GetDevice::<ID3D11Device>() }
            .is_ok_and(|actual| actual.as_raw() == device)
    {
        RENDERER_STARTUP_OBSERVED.store(true, Ordering::Release);
    }
}

unsafe extern "C" fn accept_window(user: Raw, window: Raw) -> i32 {
    i32::from(user == window)
}

unsafe extern "C" fn present_event(_: Raw, event: *const rsf_observer_present_event) {
    unsafe { rsf_native_window_present(event) };
}

unsafe extern "C" fn latency_event(_: Raw, event: *const rsf_observer_present_event) {
    unsafe { rsf_native_fg_present(event) };
}

fn cold_status() -> bool {
    let mut status = rsf_d3d11_present_status {
        struct_size: size_of::<rsf_d3d11_present_status>() as u32,
        version: RSF_PRESENT_STATUS_VERSION,
        ..Default::default()
    };
    if unsafe { rsf_d3d11_present_status_get(&mut status) } != NOT_READY || status.valid != 0 {
        return false;
    }
    let mut short = status;
    short.struct_size = size_of::<rsf_d3d11_present_status>() as u32 - 1;
    if unsafe { rsf_d3d11_present_status_get(&mut short) } != INVALID_ARGUMENT {
        return false;
    }
    status.version += 1;
    if unsafe { rsf_d3d11_present_status_get(&mut status) } != ABI_MISMATCH {
        return false;
    }
    println!("PASS: cold provider status refusals");
    true
}

struct Mode {
    borderless: bool,
    switching: bool,
    pacing: bool,
    ordering: bool,
    ten_bit: bool,
    visible: bool,
    enabled: bool,
}

impl Mode {
    fn new(argc: usize, flag: &[u8]) -> Self {
        let switching_visible = argc == 6 && flag == b"--switch-visible";
        let borderless = argc == 6 && flag == b"--borderless";
        let switching = switching_visible || borderless || (argc == 6 && flag == b"--switch");
        let pacing = argc == 4 && flag == b"--enable-visible-pacing";
        let ordering = argc == 4 && flag == b"--ordering-only";
        let ten_bit = pacing || (argc == 4 && flag == b"--enable-visible-10bit");
        let visible = switching_visible || ten_bit || (argc == 4 && flag == b"--enable-visible");
        let enabled = switching || ordering || visible || (argc == 4 && flag == b"--enable");
        Self {
            borderless,
            switching,
            pacing,
            ordering,
            ten_bit,
            visible,
            enabled,
        }
    }

    unsafe fn install(&self, window: HWND, argv: &[*const c_char]) -> bool {
        let dev = unsafe { CStr::from_ptr(argv[1]) }
            .to_bytes()
            .windows(b"development".len())
            .any(|part| part == b"development");
        let mut setup = rsf_d3d11_present_setup {
            struct_size: size_of::<rsf_d3d11_present_setup>() as u32,
            runtime_directory_utf8: argv[1],
            development_runtime: u32::from(dev),
            log: Some(rsf_ac7_fixture_log),
            user: window.0,
            accept_window: Some(accept_window),
            before_present: Some(before_present),
            present_event: Some(present_event),
            latency_event: Some(latency_event),
            retire: Some(rsf_native_fg_retire),
            prepare: Some(rsf_native_fg_prepare),
            debug_timing: 1,
            ui_mode: RSF_UI_MODE_BACKBUFFER_HUDLESS,
            depth_inverted: 1,
            depth_infinite: 1,
            units_to_meters: 0.01,
            engine_type: 1,
            engine_version_utf8: c"4.18.3".as_ptr(),
            project_id_utf8: c"a3ed1f08-3542-4698-b85c-e1a9908e861a".as_ptr(),
            ..Default::default()
        };
        if self.switching {
            setup.backend = RSF_FG_BACKEND_DLSS;
            setup.runtime_switching = 1;
            setup.max_generated_frames = u32::MAX;
            setup.streamline_directory_utf8 = argv[1];
            setup.fsr3_directory_utf8 = argv[4];
            setup.fsr4_directory_utf8 = argv[4];
            setup.xess_directory_utf8 = argv[5];
        }
        unsafe { rsf_d3d11_present_install(&setup) != 0 }
    }

    fn format(&self) -> DXGI_FORMAT {
        if self.ten_bit {
            DXGI_FORMAT_R10G10B10A2_UNORM
        } else {
            DXGI_FORMAT_R8G8B8A8_UNORM
        }
    }

    fn create_window(&self, instance: HINSTANCE) -> Result<HWND, i32> {
        if !self.borderless {
            return window(CLASS, w!("Presentation bridge fixture"), instance);
        }
        // Keep X-Ray's ANSI class, topmost style and off-origin starting position.
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 640,
            bottom: 480,
        };
        let _ = unsafe { AdjustWindowRect(&mut rect, style, false) };
        let class = s!("_XRAY_FIXTURE");
        let xray = WNDCLASSA {
            hInstance: instance,
            lpfnWndProc: Some(DefWindowProcA),
            lpszClassName: class,
            ..Default::default()
        };
        if unsafe { RegisterClassA(&xray) } == 0 {
            return Err(1);
        }
        unsafe {
            CreateWindowExA(
                WS_EX_TOPMOST,
                class,
                s!("Presentation bridge fixture"),
                style,
                190,
                190,
                rect.right - rect.left,
                rect.bottom - rect.top,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .map_err(|_| 1)
    }
}

const CLASS: windows::core::PCWSTR = w!("RSFPresentBridgeFixture");

fn engine_graphics() -> Result<rsf_streamline_graphics, i32> {
    let mut presentation = rsf_streamline_graphics {
        struct_size: size_of::<rsf_streamline_graphics>() as u32,
        ..Default::default()
    };
    let mut transfer = presentation;
    if unsafe { rsf_streamline_host_graphics(rsf_d3d11_present_host(), &mut presentation) } != 0
        || unsafe { rsf_d3d11_present_graphics(&mut transfer) } == 0
        || presentation.queue == transfer.queue
        || presentation.device != transfer.device
    {
        return Err(1);
    }
    println!("PASS: SR/upload use a separate queue on the same presentation device");
    Ok(transfer)
}

fn scene(frame: &rsf_dlss_frame) -> rsf_game_render_pass {
    let mut source: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    source.struct_size = size_of::<rsf_game_render_pass>() as u32;
    source.session_id = 42;
    source.family_key = 1;
    source.view_key = 2;
    source.viewport_key = 3;
    source.screen = RSF_SCREEN_FLIGHT;
    source.flags = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION;
    let camera = &mut source.camera;
    camera.struct_size = size_of::<rsf_camera_frame>() as u32;
    camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    camera.render_width = 512;
    camera.render_height = 512;
    camera.output_width = 512;
    camera.output_height = 512;
    camera.near_plane = 0.1;
    camera.vertical_fov_radians = 1.0;
    camera.depth_inverted = 1;
    camera.view_to_clip = frame.camera_view_to_clip;
    camera.clip_to_view = frame.clip_to_camera_view;
    camera.clip_to_previous_clip = frame.clip_to_prev_clip;
    for index in [0, 5, 10, 15] {
        camera.view_to_world[index] = 1.0;
        camera.world_to_view[index] = 1.0;
    }
    camera.frame_time_seconds = 1.0 / 60.0;
    source.output_rect[2] = 512;
    source.output_rect[3] = 512;
    source
}

fn limit(name: PCSTR) -> u32 {
    let mut bytes = [0u8; 16];
    let read = unsafe { GetEnvironmentVariableA(name, Some(&mut bytes)) };
    if read != 0 && (read as usize) < bytes.len() {
        unsafe { number(bytes.as_ptr().cast()) }
    } else {
        0
    }
}

fn resize(chain: &IDXGISwapChain, width: u32, height: u32) -> Result<(), i32> {
    unsafe {
        chain.ResizeBuffers(
            1,
            width,
            height,
            DXGI_FORMAT_UNKNOWN,
            DXGI_SWAP_CHAIN_FLAG(0),
        )
    }
    .map_err(|_| 1)
}

fn run_enabled(
    mode: &Mode,
    native: &rsf_ac7_fixture_calls,
    chain: &IDXGISwapChain,
    graphics: (&ID3D11Device, &ID3D11DeviceContext),
    hwnd: HWND,
    gpu: Raw,
    frame: &mut rsf_dlss_frame,
) -> Result<(), i32> {
    let (device, context) = graphics;
    resize(chain, 512, 512)?;
    if mode.visible {
        show(hwnd, true);
    }
    println!(
        "fixture window={:p} foreground={:p}",
        hwnd.0,
        unsafe { GetForegroundWindow() }.0
    );
    if unsafe {
        rsf_dlss_share_host(
            rsf_d3d11_present_host().cast(),
            Some(rsf_ac7_fixture_log),
            null_mut(),
        )
    } != 0
    {
        return Err(1);
    }
    let mut options = rsf_native_fg_options {
        struct_size: size_of::<rsf_native_fg_options>() as u32,
        mode: RSF_FG_FIXED,
        generated_frames: 1,
        reflex_mode: RSF_REFLEX_ON,
        frame_limit_us: limit(s!("RSF_TEST_FRAME_LIMIT_US")),
        debug: 1,
    };
    let stall = limit(s!("RSF_TEST_SUBMISSION_STALL_MS"));
    unsafe {
        rsf_native_fg_options_set(&options);
        rsf_native_fg_set_log(Some(rsf_ac7_fixture_log), null_mut());
    }
    let description = D3D11_TEXTURE2D_DESC {
        Width: 512,
        Height: 512,
        MipLevels: 1,
        ArraySize: 1,
        Format: mode.format(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
        ..Default::default()
    };
    let mut hudless = None;
    unsafe { device.CreateTexture2D(&description, None, Some(&mut hudless)) }.map_err(|_| 1)?;
    let hudless = hudless.ok_or(1)?;
    let mut target: Option<ID3D11RenderTargetView> = None;
    unsafe { device.CreateRenderTargetView(&hudless, None, Some(&mut target)) }.map_err(|_| 1)?;
    let target = target.ok_or(1)?;
    let mut source = scene(frame);
    let mut inputs = rsf_ac7_loop {
        chain: chain.as_raw(),
        window: hwnd.0,
        gpu,
        source: &mut source,
        options: &mut options,
        switching: u32::from(mode.switching),
        pacing: u32::from(mode.pacing),
        ordering_only: u32::from(mode.ordering),
        visible: u32::from(mode.visible),
        ..Default::default()
    };
    if unsafe {
        (native.run_guarded.unwrap())(
            &mut inputs,
            device.as_raw(),
            context.as_raw(),
            hudless.as_raw(),
            target.as_raw(),
            frame,
            stall,
        )
    } != 0
    {
        return Err(1);
    }
    if unsafe { rsf_dlss_release_resources() } != 0 || unsafe { rsf_dlss_shutdown() } != 0 {
        return Err(1);
    }
    Ok(())
}

unsafe fn run(argv: &[*const c_char], native: &rsf_ac7_fixture_calls) -> Result<i32, i32> {
    if !cold_status() {
        return Err(1);
    }
    let argc = argv.len();
    let arg = |i: usize| unsafe { CStr::from_ptr(argv[i]) }.to_bytes();
    if argc == 2 && arg(1) == b"--status-snapshot" {
        return Ok(0);
    }
    if argc == 3 && arg(1) == b"--proxy-smoke" {
        return Ok(unsafe { rsf_d3d11_fixture_proxy_smoke(argv[2]) });
    }
    if !matches!(argc, 3 | 4 | 6) {
        println!(
            "SKIP: supply Streamline directory, adapter vendor ID, optional --enable or --switch FSR-directory XeSS-directory"
        );
        return Ok(77);
    }
    let mode = Mode::new(argc, if argc > 3 { arg(3) } else { b"" });
    unsafe { (native.install_exception_filter.unwrap())() };
    if mode.borderless {
        let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE) };
    }
    println!("fixture: creating D3D11 graphics");
    if unsafe { GetEnvironmentVariableA(s!("RSF_TEST_D3D12_DEBUG"), None) } != 0
        && rsf_present_fixture_debug12() != 0
    {
        return Err(1);
    }
    let (factory, adapter) = adapter::<IDXGIFactory2>(unsafe { number(argv[2]) }, false)?;
    let (device, context) = device11(&adapter).map_err(|_| 77)?;
    let observer = rsf_observer_options {
        struct_size: size_of::<rsf_observer_options>() as u32,
        abi_version: RSF_OBSERVER_ABI_VERSION,
        format: DXGI_FORMAT_R16G16_FLOAT.0 as u32,
        capacity: 1,
        ..Default::default()
    };
    if unsafe { rsf_observer_install(&observer) } != 0 {
        return Err(1);
    }
    println!("fixture: D3D11 observer installed");
    let instance = register_window(CLASS)?;
    let hwnd = mode.create_window(instance)?;
    if mode.borderless {
        show(hwnd, false);
    }
    if mode.visible {
        show(hwnd, true);
    }
    if !unsafe { mode.install(hwnd, argv) } {
        return Err(1);
    }
    println!("fixture: presentation interception installed");
    let desc = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Width: if mode.borderless { 640 } else { 1280 },
            Height: if mode.borderless { 480 } else { 720 },
            Format: mode.format(),
            ..Default::default()
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: if mode.borderless { 2 } else { 1 },
        OutputWindow: hwnd,
        Windowed: BOOL(1),
        SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
        ..Default::default()
    };
    let chain = create_chain(&factory, device.as_raw(), &desc)?;
    if mode.borderless {
        let result = unsafe { rsf_d3d11_fixture_borderless(chain.as_raw(), hwnd.0) };
        drop(chain);
        let _ = unsafe { DestroyWindow(hwnd) };
        return Ok(result);
    }
    let host = unsafe { rsf_d3d11_present_host() };
    if host.is_null() {
        if unsafe {
            rsf_d3d11_fixture_clear_and_present(
                device.as_raw(),
                context.as_raw(),
                chain.as_raw(),
                0,
            )
        } != 0
        {
            return Err(1);
        }
        println!(
            "PASS fallback: original D3D11 buffer, draw and Present after FG startup refusal; FG capability skipped"
        );
        return Ok(77);
    }

    let transfer = engine_graphics()?;
    let mut textures = [null_mut(); 9];
    if unsafe { rsf_d3d11_fixture_inputs(device.as_raw(), context.as_raw(), textures.as_mut_ptr()) }
        != 0
    {
        return Err(1);
    }
    let _inputs = textures.map(|texture| unsafe { ID3D11Texture2D::from_raw(texture) });
    let mut frame = rsf_dlss_frame::default();
    if unsafe {
        rsf_d3d11_fixture_shared_sr(
            host.cast(),
            device.as_raw(),
            context.as_raw(),
            &textures,
            Some(rsf_ac7_fixture_log),
            &mut frame,
        )
    } != 0
    {
        return Err(1);
    }
    let modern = chain.cast::<IDXGISwapChain4>().map_err(|_| 1)?;
    let identity = chain.cast::<IUnknown>().map_err(|_| 1)?;
    let modern_identity = modern.cast::<IUnknown>().map_err(|_| 1)?;
    let returned = unsafe { chain.GetDevice::<ID3D11Device>() }.map_err(|_| 1)?;
    if modern_identity.as_raw() != identity.as_raw() || returned.as_raw() != device.as_raw() {
        return Err(1);
    }
    if unsafe {
        rsf_d3d11_fixture_clear_and_present(device.as_raw(), context.as_raw(), chain.as_raw(), 1)
    } != 0
    {
        return Err(1);
    }
    let (result, observed) = observer_status();
    if !RENDERER_STARTUP_OBSERVED.load(Ordering::Acquire)
        || result != 0
        || observed.frames_presented != 12
        || observed.present_width != 1280
        || observed.present_height != 720
    {
        return Err(1);
    }
    println!(
        "PASS: renderer startup sees the facade's presenting device/output and 12 source Presents"
    );
    if mode.enabled {
        run_enabled(
            &mode,
            native,
            &chain,
            (&device, &context),
            hwnd,
            transfer.native_device,
            &mut frame,
        )?;
    } else if unsafe { rsf_d3d11_marker_fixture(chain.as_raw(), &*native.marker_calls) } == 0 {
        return Err(1);
    }
    unsafe { context.ClearState() };
    resize(&chain, 1024, 576)?;
    let resized = unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }.map_err(|_| 1)?;
    let mut size = D3D11_TEXTURE2D_DESC::default();
    unsafe { resized.GetDesc(&mut size) };
    if size.Width != 1024
        || size.Height != 576
        || unsafe { chain.Present(0, DXGI_PRESENT_TEST) }.is_err()
    {
        return Err(1);
    }
    drop(resized);
    drop(returned);
    drop(modern_identity);
    drop(identity);
    drop(modern);
    // Transfer the original COM reference to the exact owned Release dispatch.
    let mut release: rsf_d3d11_chain_dispatch = unsafe { std::mem::zeroed() };
    release.method = RSF_D3D11_DISPATCH_RELEASE;
    release.receiver = chain.into_raw();
    unsafe { rsf_d3d11_chain_dispatch_owned(&mut release) };
    if release.handled == 0
        || unsafe { release.result.references } != 0
        || unsafe { rsf_d3d11_present_has_owner() } != 0
    {
        return Err(1);
    }
    let _ = unsafe { DestroyWindow(hwnd) };
    let _ = unsafe { UnregisterClassW(CLASS, Some(instance)) };
    println!(
        "PASS: shared D3D12 DLSS SR and CPU token; D3D11 COM identity/buffers, Off Present, retained-reference resize refusal, resize and retirement"
    );
    Ok(0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_fixture_main(
    argc: i32,
    argv: *const *const c_char,
    native: &rsf_ac7_fixture_calls,
) -> i32 {
    unsafe { run(args(argc, argv), native) }.unwrap_or_else(|status| status)
}
