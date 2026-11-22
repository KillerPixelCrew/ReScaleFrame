// SPDX-License-Identifier: MIT
use windows::{
    Win32::{
        Foundation::{E_POINTER, HINSTANCE, HMODULE, HWND},
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP},
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDeviceAndSwapChain,
                ID3D11Device, ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Texture2D,
            },
            Dxgi::{
                Common::{
                    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_MODE_DESC,
                    DXGI_SAMPLE_DESC,
                },
                DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_CHAIN_FLAG,
                DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter,
                IDXGIDevice, IDXGIFactory, IDXGISwapChain,
            },
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE,
            PeekMessageW, RegisterClassW, TranslateMessage, WINDOW_EX_STYLE, WNDCLASSW,
            WS_OVERLAPPEDWINDOW,
        },
    },
    core::{BOOL, Interface, PCWSTR, w},
};

const CLASS: PCWSTR = w!("RsfOverlayDllApiFixture");
struct Window(HWND);

impl Window {
    fn new(
        instance: HINSTANCE,
        title: PCWSTR,
        width: i32,
        height: i32,
    ) -> windows::core::Result<Self> {
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS,
                title,
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                width,
                height,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .map(Self)
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

fn description(window: HWND, width: u32, height: u32) -> DXGI_SWAP_CHAIN_DESC {
    DXGI_SWAP_CHAIN_DESC {
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
        BufferCount: 2,
        OutputWindow: window,
        Windowed: BOOL(1),
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        ..Default::default()
    }
}

fn create(
    desc: &DXGI_SWAP_CHAIN_DESC,
    driver: D3D_DRIVER_TYPE,
) -> windows::core::Result<(IDXGISwapChain, ID3D11DeviceContext, ID3D11Device)> {
    let (mut chain, mut device, mut context) = (None, None, None);
    unsafe {
        D3D11CreateDeviceAndSwapChain(
            None::<&IDXGIAdapter>,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(desc),
            Some(&mut chain),
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }?;
    match (chain, context, device) {
        (Some(chain), Some(context), Some(device)) => Ok((chain, context, device)),
        _ => Err(E_POINTER.into()),
    }
}

pub(super) enum StartError {
    Window(windows::core::Error),
    Device(windows::core::Error),
}

pub(super) struct Graphics {
    pub(super) chain: IDXGISwapChain,
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    window: Window,
    instance: HINSTANCE,
}

impl Graphics {
    pub(super) fn new() -> Result<Self, StartError> {
        let module = unsafe { GetModuleHandleW(PCWSTR::null()) }.map_err(StartError::Window)?;
        let instance = HINSTANCE(module.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(default_window_procedure),
            hInstance: instance,
            lpszClassName: CLASS,
            ..Default::default()
        };
        unsafe { RegisterClassW(&class) };
        let window =
            Window::new(instance, w!("overlay fixture"), 640, 360).map_err(StartError::Window)?;
        let desc = description(window.0, 640, 360);
        let ((chain, context, device), adapter) = create(&desc, D3D_DRIVER_TYPE_HARDWARE)
            .map(|objects| (objects, "hardware"))
            .or_else(|_| create(&desc, D3D_DRIVER_TYPE_WARP).map(|objects| (objects, "WARP")))
            .map_err(StartError::Device)?;
        println!("D3D11 {adapter} device, hidden 640x360 window");
        Ok(Self {
            chain,
            context,
            device,
            window,
            instance,
        })
    }

    pub(super) fn hwnd(&self) -> HWND {
        self.window.0
    }

    pub(super) fn clear(&self) -> windows::core::Result<()> {
        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let back: ID3D11Texture2D = unsafe { self.chain.GetBuffer(0) }?;
        let mut view: Option<ID3D11RenderTargetView> = None;
        unsafe {
            self.device
                .CreateRenderTargetView(&back, None, Some(&mut view))
        }?;
        drop(back);
        let view = view.ok_or(E_POINTER)?;
        unsafe {
            self.context
                .ClearRenderTargetView(&view, &[0.1, 0.2, 0.3, 1.0])
        };
        Ok(())
    }

    pub(super) fn submit(&self) -> bool {
        unsafe { self.chain.Present(0, DXGI_PRESENT(0)) }.is_ok()
    }

    pub(super) fn resize(&self, width: u32, height: u32) -> bool {
        unsafe {
            self.chain.ResizeBuffers(
                0,
                width,
                height,
                DXGI_FORMAT_UNKNOWN,
                DXGI_SWAP_CHAIN_FLAG(0),
            )
        }
        .is_ok()
    }

    pub(super) fn with_foreign_chain(
        &self,
        run: impl FnOnce(&IDXGISwapChain),
    ) -> windows::core::Result<()> {
        let window = Window::new(self.instance, w!("other"), 64, 64)?;
        let desc = description(window.0, 64, 64);
        let dxgi = self.device.cast::<IDXGIDevice>()?;
        let adapter = unsafe { dxgi.GetAdapter() }?;
        let factory: IDXGIFactory = unsafe { adapter.GetParent() }?;
        let mut chain = None;
        unsafe { factory.CreateSwapChain(&self.device, &desc, &mut chain) }.ok()?;
        run(&chain.ok_or(E_POINTER)?);
        Ok(())
    }
}

// The windows crate wraps DefWindowProcW in a Rust function; the class needs the system ABI.
unsafe extern "system" fn default_window_procedure(
    window: windows::Win32::Foundation::HWND,
    message: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}
