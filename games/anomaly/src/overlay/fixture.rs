// SPDX-License-Identifier: MIT
//! Link the Windows fixtures to the same adapter entry points used by the carrier.
use super::*;
use crate::input::gate;
use windows::Win32::Devices::HumanInterfaceDevice::*;
use windows::Win32::Foundation::{GENERIC_READ, HWND};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::{Common::*, *};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::core::{Interface, PCWSTR};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

struct Window(HWND);
impl Window {
    fn new(class: *const u16, style: u32, width: i32, height: i32) -> Option<Self> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        let wc = WNDCLASSW {
            lpfnWndProc: Some(DefWindowProcW),
            hInstance: instance,
            lpszClassName: class,
            ..Default::default()
        };
        unsafe { RegisterClassW(&wc) };
        let window = unsafe {
            CreateWindowExW(
                0,
                class,
                w!("Anomaly fixture"),
                style,
                0,
                0,
                width,
                height,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null(),
            )
        };
        (!window.is_null()).then_some(Self(HWND(window)))
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.0.0) };
    }
}

struct Graphics {
    chain: IDXGISwapChain,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
}
impl Graphics {
    fn new(
        window: HWND,
        driver: D3D_DRIVER_TYPE,
        width: u32,
        height: u32,
        flip: bool,
    ) -> Option<Self> {
        let desc = DXGI_SWAP_CHAIN_DESC {
            BufferDesc: DXGI_MODE_DESC {
                Width: width,
                Height: height,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                ..Default::default()
            },
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: if flip { 2 } else { 1 },
            OutputWindow: window,
            Windowed: true.into(),
            SwapEffect: if flip {
                DXGI_SWAP_EFFECT_FLIP_DISCARD
            } else {
                DXGI_SWAP_EFFECT_DISCARD
            },
            ..Default::default()
        };
        let (mut chain, mut device, mut context) = (None, None, None);
        unsafe {
            D3D11CreateDeviceAndSwapChain(
                None::<&IDXGIAdapter>,
                driver,
                Default::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&desc),
                Some(&mut chain),
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .ok()?;
        Some(Self {
            chain: chain?,
            device: device?,
            context: context?,
        })
    }
}

#[derive(Default)]
struct Checks(u32);
impl Checks {
    fn check(&mut self, condition: bool, message: &str) {
        if !condition {
            eprintln!("FAIL: {message}");
            self.0 += 1;
        }
    }
    fn exit(&self) -> i32 {
        i32::from(self.0 != 0)
    }
}

static PRESENTED: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static NOTIFIED: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static TOGGLES: AtomicU32 = AtomicU32::new(0);
static CAPTURE_QUERIES: AtomicU32 = AtomicU32::new(0);
static CAPTURE_BITS: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn configure(_: *const rsf_overlay_service_setup) -> i32 {
    OK
}
unsafe extern "C" fn present(chain: *mut c_void, drawn: *mut u32) -> i32 {
    lock(&PRESENTED).push(chain.addr());
    if let Some(drawn) = unsafe { drawn.as_mut() } {
        *drawn = 0;
    }
    OK
}
unsafe extern "C" fn notify(_: *mut c_void, reason: u32) -> i32 {
    lock(&NOTIFIED).push(reason);
    OK
}
unsafe extern "C" fn visible() -> u32 {
    0
}
unsafe extern "C" fn set_visible(_: u32) -> i32 {
    OK
}
unsafe extern "C" fn toggle() -> i32 {
    TOGGLES.fetch_add(1, Ordering::Relaxed);
    OK
}
unsafe extern "C" fn capture(out: *mut rsf_overlay_capture) -> i32 {
    CAPTURE_QUERIES.fetch_add(1, Ordering::Relaxed);
    unsafe { (*out).captured = CAPTURE_BITS.load(Ordering::Relaxed) };
    OK
}
unsafe extern "C" fn status(_: *mut rsf_render_status) -> i32 {
    OK
}
unsafe extern "C" fn settings_get(_: *mut rsf_render_settings) -> i32 {
    OK
}
unsafe extern "C" fn settings_apply(_: *const rsf_render_settings) -> i32 {
    OK
}

fn dxgi(checks: &mut Checks) -> Option<()> {
    let engine = Window::new(w!("_XRAY_1.5"), WS_POPUP, 64, 64)?;
    let other = Window::new(w!("RSF_NOT_THE_ENGINE"), WS_POPUP, 64, 64)?;
    let game = Graphics::new(engine.0, D3D_DRIVER_TYPE_WARP, 64, 64, false)?;
    let side = Graphics::new(other.0, D3D_DRIVER_TYPE_WARP, 64, 64, false)?;
    let first = unsafe { game.chain.Present(0, DXGI_PRESENT(0)) };
    checks.check(first.is_ok(), "engine chain presents");
    checks.check(
        lock(&PRESENTED).first() == Some(&game.chain.as_raw().addr()),
        "overlay presented on the engine chain",
    );
    let count = lock(&PRESENTED).len();
    unsafe {
        let _ = game.chain.Present(0, DXGI_PRESENT_TEST);
        let _ = side.chain.Present(0, DXGI_PRESENT(0));
    }
    checks.check(
        lock(&PRESENTED).len() == count,
        "test and unrelated Presents did not reach the overlay",
    );
    checks.check(
        lock(&NOTIFIED).is_empty(),
        "the first chain did not notify a reset",
    );
    checks.check(
        TOGGLES.load(Ordering::Relaxed) == 0,
        "no Insert press or fallback toggle",
    );
    checks.check(
        unsafe {
            side.chain
                .ResizeBuffers(1, 32, 32, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))
        }
        .is_ok(),
        "unrelated ResizeBuffers forwards",
    );
    checks.check(
        lock(&NOTIFIED).is_empty(),
        "unrelated resize did not retire the engine's panel",
    );
    checks.check(
        unsafe {
            game.chain
                .ResizeBuffers(1, 32, 32, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))
        }
        .is_ok(),
        "engine ResizeBuffers forwards",
    );
    checks.check(
        *lock(&NOTIFIED) == [RSF_OVERLAY_NOTIFY_RESIZE],
        "engine resize notified once",
    );
    let second = Graphics::new(engine.0, D3D_DRIVER_TYPE_WARP, 64, 64, false)?;
    lock(&NOTIFIED).clear();
    unsafe {
        let _ = second.chain.Present(0, DXGI_PRESENT(0));
    }
    checks.check(
        *lock(&NOTIFIED) == [RSF_OVERLAY_NOTIFY_DEVICE_RESET],
        "replacement chain notified device reset",
    );
    checks.check(
        lock(&PRESENTED).last() == Some(&second.chain.as_raw().addr()),
        "overlay followed the replacement chain",
    );
    Some(())
}

fn directinput(checks: &mut Checks, format: *const DIDATAFORMAT) -> Option<()> {
    let window = Window::new(w!("RSF_DINPUT_TEST"), WS_POPUP, 64, 64)?;
    let module = unsafe { crate::runtime_module::Module::system(w!("dinput8.dll")) }?;
    let create = module.symbol(c"DirectInput8Create")?;
    let iid = windows_sys::core::GUID::from_u128(0xbf798030_483a_4da2_aa99_5d64ed369700);
    let raw = unsafe { overlay_native::create_input(create, &iid, 0x0800) };
    if raw.is_null() {
        return None;
    }
    let input = unsafe { IDirectInput8A::from_raw(raw) };
    let mut mouse = None;
    let mut keyboard = None;
    unsafe {
        input.CreateDevice(&GUID_SysMouse, &mut mouse, None).ok()?;
        input
            .CreateDevice(&GUID_SysKeyboard, &mut keyboard, None)
            .ok()?;
    }
    let mouse = mouse?;
    let keyboard = keyboard?;
    let tracked =
        |device| overlay_hooks::active_gate().is_some_and(|gate| gate::tracked(gate, device));
    checks.check(tracked(mouse.as_raw()), "mouse registered with the gate");
    checks.check(
        tracked(keyboard.as_raw()),
        "keyboard registered with the gate",
    );
    unsafe {
        mouse.SetDataFormat(format.cast_mut()).ok()?;
        mouse
            .SetCooperativeLevel(window.0, DISCL_NONEXCLUSIVE | DISCL_BACKGROUND)
            .ok()?;
        mouse.Acquire().ok()?;
    }
    let mut state = DIMOUSESTATE::default();
    CAPTURE_BITS.store(0, Ordering::Relaxed);
    CAPTURE_QUERIES.store(0, Ordering::Relaxed);
    let read = |state: &mut DIMOUSESTATE| unsafe {
        mouse.GetDeviceState(
            size_of::<DIMOUSESTATE>() as u32,
            ptr::from_mut(state).cast(),
        )
    };
    checks.check(
        read(&mut state).is_ok(),
        "closed panel forwards real mouse state",
    );
    checks.check(
        CAPTURE_QUERIES.load(Ordering::Relaxed) > 0,
        "mouse read consulted the gate",
    );
    CAPTURE_BITS.store(RSF_OVERLAY_CAPTURE_MOUSE, Ordering::Relaxed);
    state = DIMOUSESTATE {
        lX: -1,
        lY: -1,
        lZ: -1,
        rgbButtons: [0xab; 4],
    };
    checks.check(
        read(&mut state).is_ok()
            && [state.lX, state.lY, state.lZ] == [0; 3]
            && state.rgbButtons == [0; 4],
        "captured mouse is idle",
    );
    let mut events = [DIDEVICEOBJECTDATA::default(); 16];
    let mut count = events.len() as u32;
    let result = unsafe {
        mouse.GetDeviceData(
            size_of::<DIDEVICEOBJECTDATA>() as u32,
            events.as_mut_ptr(),
            &mut count,
            0,
        )
    };
    checks.check(
        result.is_ok() && count == 0,
        "captured mouse withholds buffered events",
    );
    CAPTURE_BITS.store(0, Ordering::Relaxed);
    unsafe {
        let _ = mouse.Unacquire();
    }
    Some(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_overlay_hooks_fixture(
    hooks: &HookOps,
    format: *const DIDATAFORMAT,
) -> i32 {
    // The C host supplies the pinned MinHook calls and the SDK mouse format.
    let api = rsf_overlay_api {
        struct_size: size_of::<rsf_overlay_api>() as u32,
        abi_version: RSF_OVERLAY_API_VERSION,
        configure: Some(configure),
        present: Some(present),
        notify: Some(notify),
        visible: Some(visible),
        set_visible: Some(set_visible),
        toggle: Some(toggle),
        capture: Some(capture),
        status: Some(status),
        settings_get: Some(settings_get),
        settings_apply: Some(settings_apply),
    };
    let options = rsf_anomaly_overlay_options {
        configuration: w!(r"C:\rsf-test\ReScaleFrame.ini"),
        api_override: &api,
        install_hooks: 1,
        ..Default::default()
    };
    let mut reason = ptr::null();
    let start = unsafe { rsf_anomaly_overlay_start(&options, Some(hooks), &mut reason) };
    if start != STARTED {
        let message = text(reason);
        eprintln!("Anomaly overlay hook start: {message}");
        return if start == HOOKS_REFUSED && message.contains("dummy swap chain") {
            77
        } else {
            1
        };
    }
    let mut checks = Checks::default();
    checks.check(
        unsafe { rsf_anomaly_overlay_start(&options, Some(hooks), &mut reason) } == ALREADY_STARTED,
        "hooks install once per process",
    );
    if dxgi(&mut checks).is_none() {
        return if checks.0 == 0 { 77 } else { 1 };
    }
    if directinput(&mut checks, format).is_none() {
        println!("DirectInput skipped because a mouse, keyboard, or acquisition was unavailable");
    }
    checks.exit()
}

const _: () = {
    use std::mem::offset_of;

    assert!(size_of::<HookOps>() == 40);
    assert!(offset_of!(HookOps, create) == 8);
    assert!(offset_of!(HookOps, remove_unqueued) == 16);
    assert!(offset_of!(HookOps, queue) == 24);
    assert!(offset_of!(HookOps, apply) == 32);
};

struct Com;
impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

struct TempRoot(PathBuf);
impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Cursor(Option<POINT>);
impl Drop for Cursor {
    fn drop(&mut self) {
        if let Some(point) = self.0 {
            unsafe { SetCursorPos(point.x, point.y) };
        }
    }
}

static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());
unsafe extern "C" fn record(_: *mut c_void, message: *const c_char) {
    let message = text(message);
    println!("  log: {message}");
    lock(&LOG).push(message);
}

fn logged(message: &str) -> bool {
    lock(&LOG).iter().any(|line| line.contains(message))
}

fn pump() {
    let mut message = MSG::default();
    unsafe {
        while PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn frames(graphics: &Graphics, count: usize) -> windows::core::Result<()> {
    for _ in 0..count {
        pump();
        unsafe {
            let back: ID3D11Texture2D = graphics.chain.GetBuffer(0)?;
            let mut view = None;
            graphics
                .device
                .CreateRenderTargetView(&back, None, Some(&mut view))?;
            if let Some(view) = view {
                graphics
                    .context
                    .ClearRenderTargetView(&view, &[0.1, 0.2, 0.3, 1.0]);
            }
        }
        rsf_anomaly_overlay_present(ptr::null_mut(), graphics.chain.as_raw());
        unsafe { graphics.chain.Present(0, DXGI_PRESENT(0)) }.ok()?;
    }
    Ok(())
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

fn differing_pixels(path: &Path) -> windows::core::Result<(u32, u32, usize)> {
    let file = wide(path);
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let decoder = factory.CreateDecoderFromFilename(
            PCWSTR(file.as_ptr()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )?;
        let source = decoder.GetFrame(0)?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &source,
            &GUID_WICPixelFormat32bppBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let (mut width, mut height) = (0, 0);
        converter.GetSize(&mut width, &mut height)?;
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        converter.CopyPixels(ptr::null(), width * 4, &mut pixels)?;
        let count = pixels
            .chunks_exact(4)
            .filter(|bgra| {
                bgra[2].abs_diff(26) > 3 || bgra[1].abs_diff(51) > 3 || bgra[0].abs_diff(77) > 3
            })
            .count();
        Ok((width, height, count))
    }
}

fn visibility(query: rsf_get_overlay_api_fn) -> Result<i32, Box<dyn std::error::Error>> {
    let panel = std::env::var_os("RSF_OVERLAY_DLL").ok_or("RSF_OVERLAY_DLL is missing")?;
    if !Path::new(&panel).is_file() {
        return Err("RSF_OVERLAY_DLL must name the built panel".into());
    }
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
    let _com = Com;
    let runtime = unsafe { GetModuleHandleW(w!("ReScaleFrame.Runtime.dll")) };
    if runtime.is_null() {
        return Err("the imported Runtime DLL is absent".into());
    }
    let window = Window::new(w!("_XRAY_1.5"), WS_OVERLAPPEDWINDOW, 640, 360)
        .ok_or("creating the hidden engine window failed")?;
    let Some(gpu) = Graphics::new(window.0, D3D_DRIVER_TYPE_HARDWARE, 640, 360, true)
        .or_else(|| Graphics::new(window.0, D3D_DRIVER_TYPE_WARP, 640, 360, true))
    else {
        println!("SKIP: no D3D11 device");
        return Ok(77);
    };

    let root = std::env::temp_dir().join(format!(
        "rsf_anomaly_overlay_visibility_{}",
        std::process::id()
    ));
    std::fs::create_dir(&root)?;
    let root = TempRoot(root);
    let ini = root.0.join("ReScaleFrame.ini");
    let captures = root.0.join("captures-🧪");
    let mut units = vec![0xfeffu16];
    units.extend("[Anomaly]\r\nOverlay=1\r\nOverlayKeyPoll=0\r\nOverlayCapture=1\r\nOverlayCaptureFrames=3\r\nOverlayCaptureStride=4\r\nOverlayCaptureCycles=1\r\nOverlayCaptureDir=".encode_utf16());
    let directory: Vec<_> = captures.as_os_str().encode_wide().collect();
    units.extend(rsf_game_sys::profile_text::encode(&directory));
    units.extend("\r\n".encode_utf16());
    std::fs::write(
        &ini,
        units
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )?;
    let ini = wide(&ini);
    let options = rsf_anomaly_overlay_options {
        runtime,
        configuration: ini.as_ptr(),
        log: Some(record),
        ..Default::default()
    };
    let mut reason = ptr::null();
    if unsafe { rsf_anomaly_overlay_start(&options, None, &mut reason) } != STARTED {
        return Err(format!("adapter start failed: {}", text(reason)).into());
    }

    let mut checks = Checks::default();
    checks.check(
        logged("OverlayCapture") || logged("capture armed"),
        "capture arming is logged",
    );
    let mut table = rsf_overlay_api {
        struct_size: size_of::<rsf_overlay_api>() as u32,
        abi_version: RSF_OVERLAY_API_VERSION,
        ..Default::default()
    };
    let query = query.ok_or("missing Runtime overlay import")?;
    let api = validate(unsafe { query(&mut table) }, &table)
        .map_err(|refusal| format!("Runtime overlay API: {}", refusal.text()))?;
    frames(&gpu, 3)?;
    checks.check(
        !captures.join("open01_f00.png").exists(),
        "no capture before opening",
    );

    std::thread::sleep(Duration::from_millis(300));
    let mut saved = POINT::default();
    let _cursor = Cursor((unsafe { GetCursorPos(&mut saved) } != 0).then_some(saved));
    let mut click = POINT { x: 120, y: 80 };
    checks.check(
        unsafe {
            ClientToScreen(window.0.0, &mut click) != 0 && SetCursorPos(click.x, click.y) != 0
        },
        "position the cursor before opening",
    );
    checks.check(
        unsafe { (api.set_visible)(1) == OK && (api.visible)() == 1 },
        "open the panel",
    );
    let pointer = 120 | (80 << 16);
    unsafe {
        SendMessageW(window.0.0, WM_MOUSEMOVE, 0, pointer);
        SendMessageW(window.0.0, WM_LBUTTONDOWN, 1, pointer);
    }
    frames(&gpu, 2)?;
    unsafe { SendMessageW(window.0.0, WM_LBUTTONUP, 0, pointer) };
    frames(&gpu, 10)?;

    let mut settings = rsf_render_settings {
        struct_size: size_of::<rsf_render_settings>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        ..Default::default()
    };
    checks.check(
        unsafe { (api.settings_get)(&mut settings) } == OK,
        "settings_get",
    );
    let refused = rsf_render_settings {
        sr_enabled: 1,
        ..settings
    };
    let invalid = rsf_game_sys::RSF_ERROR_INVALID_ARGUMENT;
    checks.check(
        unsafe { table.settings_apply.unwrap()(&refused) } == invalid,
        "enabling unavailable SR is refused",
    );
    settings.status_overlay = 1;
    checks.check(
        unsafe { table.settings_apply.unwrap()(&settings) } == OK,
        "valid settings applied",
    );
    checks.check(
        logged("from api: SR=1") && logged(&format!("refused ({invalid})")),
        "refused settings logged",
    );
    checks.check(
        logged("from api: SR=0") && logged("applied and saved (0)"),
        "saved settings logged",
    );
    settings.status_overlay = 0;
    checks.check(
        unsafe { table.settings_apply.unwrap()(&settings) } == OK,
        "restore settings",
    );

    std::thread::sleep(Duration::from_millis(300));
    checks.check(
        unsafe { (api.set_visible)(0) == OK && (api.visible)() == 0 },
        "close the panel",
    );
    frames(&gpu, 10)?;
    checks.check(
        logged("panel #1 opened: target 640x360"),
        "panel open and target logged",
    );
    checks.check(
        logged("panel #1 frame 1: api.present result 0, drawn 1"),
        "drawn state logged",
    );
    checks.check(
        logged("panel #1 closed: present drawn"),
        "close summary logged",
    );
    let counted = lock(&LOG).iter().any(|line| {
        let Some((prefix, counters)) = line.split_once("pointer events received ") else {
            return false;
        };
        let mut words = counters.split_whitespace();
        prefix.contains("panel #1 closed")
            && words
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .is_some_and(|count| count >= 3)
            && words.next() == Some("consumed")
            && words
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .is_some_and(|count| count >= 2)
    });
    checks.check(
        counted,
        "close summary records pointer reception and consumption",
    );

    let mut most = 0;
    for index in 0..3 {
        let open = differing_pixels(&captures.join(format!("open01_f{index:02}.png")))?;
        checks.check(
            (open.0, open.1) == (640, 360),
            "open capture matches the backbuffer",
        );
        most = most.max(open.2);
        let closed = differing_pixels(&captures.join(format!("close01_f{index:02}.png")))?;
        checks.check(
            closed == (640, 360, 0),
            "closed capture matches the background",
        );
    }
    checks.check(most > 3000, "open capture contains panel pixels");
    checks.check(
        !captures.join("open02_f00.png").exists(),
        "capture respects the cycle limit",
    );
    unsafe { (api.notify)(gpu.chain.as_raw(), RSF_OVERLAY_NOTIFY_DEVICE_RESET) };
    Ok(checks.exit())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_overlay_visibility_fixture(query: rsf_get_overlay_api_fn) -> i32 {
    match visibility(query) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("Anomaly overlay visibility: {error}");
            1
        }
    }
}
