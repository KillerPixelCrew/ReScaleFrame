// SPDX-License-Identifier: MIT
use super::{Handle, check, require, send, window};
use crate::sys::*;
use rsf_test_support::{d3d11_device, d3d11_swapchain};
use std::cell::RefCell;
use std::ffi::{CStr, c_char};
use std::os::windows::ffi::OsStrExt;
use std::path::absolute;
use std::ptr::null_mut;
use std::thread::sleep;
use std::time::Duration;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::{
                D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_READ,
                D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, ID3D11DepthStencilView, ID3D11Device,
                ID3D11DeviceContext, ID3D11Texture2D,
            },
            Dxgi::{
                Common::{
                    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_MODE_DESC,
                    DXGI_SAMPLE_DESC,
                },
                DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_CHAIN_FLAG,
                DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGISwapChain,
            },
        },
        System::LibraryLoader::LoadLibraryW,
        UI::{
            Input::KeyboardAndMouse::VK_INSERT,
            WindowsAndMessaging::{
                GetClientRect, SetMessageExtraInfo, ShowCursor, WM_KEYDOWN, WM_KEYUP,
                WM_LBUTTONDOWN, WM_LBUTTONUP,
            },
        },
    },
    core::{BOOL, Interface, PCWSTR, w},
};

// Cast macros in d3d11_observer.h are not emitted by bindgen.
const OBSERVER_NO_DEVICE: i32 = -4;

struct State {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    frames: usize,
    colored_frames: usize,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

unsafe extern "C" fn log_line(_: Handle, message: *const c_char) {
    eprintln!("{}", unsafe { CStr::from_ptr(message) }.to_string_lossy());
}

fn visible() -> bool {
    unsafe { rsf_overlay_host_visible() != 0 }
}

fn touch_and_cursor(window: HWND) {
    let insert = usize::from(VK_INSERT.0);
    require(!visible(), "panel starts closed");
    send(window, WM_KEYDOWN, insert, 1);
    require(visible(), "Insert opens the actual panel");
    send(window, WM_KEYDOWN, insert, 0x4000_0001);
    require(visible(), "holding Insert must not close it");
    send(window, WM_KEYUP, insert, 0xc000_0001);

    // Windows-promoted touch uses absolute coordinates and both edges can arrive in one frame.
    let mut touch = rsf_overlay_input {
        struct_size: size_of::<rsf_overlay_input>() as u32,
        ..Default::default()
    };
    require(
        unsafe { rsf_overlay_input_collect(&mut touch, 800, 600, 0.016) } == 0,
        "seed touch display extent",
    );
    let previous = unsafe { SetMessageExtraInfo(LPARAM(0xff515780)) };
    let position = 123 | (234 << 16);
    send(window, WM_LBUTTONDOWN, 1, position);
    send(window, WM_LBUTTONUP, 0, position);
    unsafe { SetMessageExtraInfo(previous) };
    require(
        unsafe { rsf_overlay_input_collect(&mut touch, 800, 600, 0.016) } == 0,
        "collect quick touch press",
    );
    let mut client = RECT::default();
    check(
        unsafe { GetClientRect(window, &mut client) },
        "get touch client rectangle",
    );
    require(
        (touch.mouse_x - 123.0 * 800.0 / client.right as f32).abs() < 1.0
            && (touch.mouse_y - 234.0 * 600.0 / client.bottom as f32).abs() < 1.0,
        "touch uses scaled contact position",
    );
    require(
        touch.mouse_buttons & RSF_OVERLAY_MOUSE_LEFT != 0,
        "tap press survives until a rendered frame",
    );
    require(
        unsafe { rsf_overlay_input_collect(&mut touch, 800, 600, 0.016) } == 0
            && touch.mouse_buttons & RSF_OVERLAY_MOUSE_LEFT == 0,
        "next frame samples tap release",
    );

    sleep(Duration::from_millis(260));
    send(window, WM_KEYDOWN, insert, 1);
    require(!visible(), "second Insert closes the panel");
    send(window, WM_KEYUP, insert, 0xc000_0001);
    sleep(Duration::from_millis(260));
    send(window, WM_KEYDOWN, insert, 1);
    require(visible(), "Insert reopens the panel for rendering");
    send(window, WM_KEYUP, insert, 0xc000_0001);

    for _ in 0..16 {
        require(
            (0..512).any(|_| unsafe { ShowCursor(false) } < 0),
            "game cursor hide loop terminates while overlay remains visible",
        );
        require(
            (0..512).any(|_| unsafe { ShowCursor(true) } >= 0),
            "game cursor show loop terminates while overlay remains visible",
        );
    }
}

fn count_pixels(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    buffer: &ID3D11Texture2D,
    mut desc: D3D11_TEXTURE2D_DESC,
) -> usize {
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.MiscFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    let mut staging = None;
    check(
        unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging)) },
        "readback allocation",
    );
    let staging = staging.expect("readback texture");
    unsafe { context.CopyResource(&staging, buffer) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    check(
        unsafe { context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) },
        "readback map",
    );
    let mut colored = 0;
    for y in 0..desc.Height as usize {
        let row = unsafe {
            std::slice::from_raw_parts(
                mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize),
                desc.Width as usize * 4,
            )
        };
        colored += row
            .chunks_exact(4)
            .filter(|pixel| pixel[..3] != [0, 0, 0])
            .count();
    }
    unsafe { context.Unmap(&staging, 0) };
    colored
}

impl State {
    fn present(&mut self, chain: &IDXGISwapChain) {
        if self.frames == 0 {
            let mut observed = null_mut();
            require(
                unsafe { rsf_observer_acquire_device(&mut observed, null_mut()) } == 0,
                "observer must select a device",
            );
            let observed = unsafe { ID3D11Device::from_raw(observed) };
            require(
                observed.as_raw() == self.device.as_raw(),
                "observer adopts the presenting device",
            );
            drop(observed);
            require(
                unsafe { rsf_overlay_host_start(chain.as_raw(), Some(log_line), null_mut()) } != 0,
                "host starts without DLSS",
            );
            require(!visible(), "panel starts closed");
        }
        if self.frames == 2 {
            let desc = check(unsafe { chain.GetDesc() }, "get input window");
            touch_and_cursor(desc.OutputWindow);
        }
        let buffer: ID3D11Texture2D = check(unsafe { chain.GetBuffer(0) }, "get back buffer");
        let mut clear_view = None;
        check(
            unsafe {
                self.device
                    .CreateRenderTargetView(&buffer, None, Some(&mut clear_view))
            },
            "clear view",
        );
        unsafe {
            self.context.ClearRenderTargetView(
                &clear_view.expect("clear render target"),
                &[0.0, 0.0, 0.0, 1.0],
            );
            self.context
                .OMSetRenderTargets(None, None::<&ID3D11DepthStencilView>);
        }
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { buffer.GetDesc(&mut desc) };
        let stats = rsf_overlay_stats {
            struct_size: size_of::<rsf_overlay_stats>() as u32,
            output_width: desc.Width,
            output_height: desc.Height,
            backend_name: c"DLSS".as_ptr(),
            refusal_reason: c"No backend in this test".as_ptr(),
            ..Default::default()
        };
        let mut intent = rsf_overlay_intent::default();
        require(
            unsafe { rsf_overlay_host_present(chain.as_raw(), &stats, &mut intent) } != 0,
            "draw on presenting device",
        );
        if self.frames < 2 {
            require(!visible(), "startup hint must not open the panel");
            require(
                intent.quality_changed == 0
                    && intent.enabled_changed == 0
                    && intent.backend_changed == 0,
                "startup hint must not request settings changes",
            );
        }
        let mut bound = [None];
        unsafe { self.context.OMGetRenderTargets(Some(&mut bound), None) };
        require(
            bound[0].is_none(),
            "restore the game's unbound render target",
        );

        let colored = count_pixels(&self.device, &self.context, &buffer, desc);
        self.colored_frames += usize::from(colored > 20);
        eprintln!(
            "frame {}: {colored} nonblack pixels at {}x{}",
            self.frames, desc.Width, desc.Height
        );
        self.frames += 1;
    }
}

unsafe extern "C" fn present(_: Handle, raw: Handle) {
    let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&raw) }.expect("Present swap chain");
    STATE.with_borrow_mut(|state| state.as_mut().expect("fixture state").present(chain));
}

fn panel_path() {
    if let Some(capture) = std::env::var_os("RSF_TEST_RENDERDOC_DLL") {
        let capture: Vec<_> = capture.encode_wide().chain([0]).collect();
        check(
            unsafe { LoadLibraryW(PCWSTR(capture.as_ptr())) },
            "load optional RenderDoc",
        );
    }
    let mut args = std::env::args_os();
    require(
        args.len() == 2,
        "pass the panel DLL filename or Windows path",
    );
    let path = check(
        absolute(args.nth(1).expect("panel DLL path")),
        "absolute panel path",
    );
    // This fixture is the only thread reading or writing the Rust environment.
    unsafe { std::env::set_var("RSF_OVERLAY_DLL", path) };
}

fn make_helper_device() {
    let (helper, _) = check(
        d3d11_device(D3D_DRIVER_TYPE_HARDWARE, D3D11_CREATE_DEVICE_FLAG(0), None),
        "create helper device",
    );
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: 16,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        ..Default::default()
    };
    let mut constant = None;
    check(
        unsafe { helper.CreateBuffer(&desc, None, Some(&mut constant)) },
        "helper creates first buffer",
    );
    require(constant.is_some(), "constant buffer");
}

fn presenting_chain(window: HWND) -> (IDXGISwapChain, State) {
    let desc = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Width: 800,
            Height: 600,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ..Default::default()
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        OutputWindow: window,
        Windowed: BOOL(1),
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        ..Default::default()
    };
    let (device, context, chain) = check(
        d3d11_swapchain(D3D_DRIVER_TYPE_HARDWARE, D3D11_CREATE_DEVICE_FLAG(0), &desc),
        "create presenting device and flip chain",
    );
    (
        chain,
        State {
            device,
            context,
            frames: 0,
            colored_frames: 0,
        },
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_overlay_host() -> i32 {
    panel_path();
    let options = rsf_observer_options {
        struct_size: size_of::<rsf_observer_options>() as u32,
        abi_version: RSF_OBSERVER_ABI_VERSION,
        capacity: 8,
        constant_buffer_min_bytes: 1024,
        constant_buffer_max_bytes: 8192,
        on_present: Some(present),
        ..Default::default()
    };
    let installed = unsafe { rsf_observer_install(&options) };
    if installed == OBSERVER_NO_DEVICE {
        eprintln!("No D3D11 device, skipping");
        return 77;
    }
    require(installed == 0, "install observer");
    make_helper_device();

    let window = window(w!("RsfOverlayHostTest"), 800, 600);
    let (chain, state) = presenting_chain(window.handle);
    STATE.with_borrow_mut(|current| *current = Some(state));
    let colored_frames =
        || STATE.with_borrow(|state| state.as_ref().expect("fixture state").colored_frames);
    for _ in 0..2 {
        let previous = colored_frames();
        for _ in 0..8 {
            check(
                unsafe { chain.Present(0, DXGI_PRESENT(0)) }.ok(),
                "forward real Present",
            );
        }
        require(
            colored_frames() >= previous + 6,
            "panel alters actual back-buffer pixels",
        );
        check(
            unsafe {
                chain.ResizeBuffers(2, 960, 720, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))
            },
            "overlay references do not prevent resize",
        );
    }

    let frames = STATE.with_borrow(|state| state.as_ref().expect("fixture state").frames);
    require(frames == 16, "all callbacks reached");
    unsafe { rsf_overlay_host_stop() };
    require(
        unsafe { rsf_observer_uninstall() } == 0,
        "uninstall observer",
    );
    STATE.with_borrow_mut(|state| drop(state.take()));
    0
}
