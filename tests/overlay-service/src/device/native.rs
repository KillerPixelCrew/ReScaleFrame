// SPDX-License-Identifier: MIT
use windows::{
    Win32::{
        Foundation::{E_POINTER, HWND},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, ID3D11Device, ID3D11DeviceContext,
                ID3D11RenderTargetView, ID3D11Texture2D,
            },
            Dxgi::{
                Common::{
                    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_MODE_DESC,
                    DXGI_SAMPLE_DESC,
                },
                DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_CHAIN_FLAG,
                DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice,
                IDXGIFactory, IDXGISwapChain,
            },
        },
        UI::WindowsAndMessaging::{
            DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WS_OVERLAPPEDWINDOW,
        },
    },
    core::{BOOL, Interface, w},
};

use rsf_test_support::{Window, d3d11_swapchain, warp_swapchain};

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

pub(super) enum StartError {
    Window(windows::core::Error),
    Device(windows::core::Error),
}

pub(super) struct Graphics {
    pub(super) chain: IDXGISwapChain,
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    window: Window,
}

impl Graphics {
    pub(super) fn new() -> Result<Self, StartError> {
        let window = Window::new(
            w!("RsfOverlayDllApiFixture"),
            w!("overlay fixture"),
            WS_OVERLAPPEDWINDOW,
            640,
            360,
        )
        .map_err(StartError::Window)?;
        let desc = description(window.handle, 640, 360);
        let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
        let ((device, context, chain), adapter) =
            d3d11_swapchain(D3D_DRIVER_TYPE_HARDWARE, flags, &desc)
                .map(|objects| (objects, "hardware"))
                .or_else(|_| warp_swapchain(flags, &desc).map(|objects| (objects, "WARP")))
                .map_err(StartError::Device)?;
        println!("D3D11 {adapter} device, hidden 640x360 window");
        Ok(Self {
            chain,
            context,
            device,
            window,
        })
    }

    pub(super) fn hwnd(&self) -> HWND {
        self.window.handle
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
        let window = Window::new(
            w!("RsfOverlayDllApiFixtureOther"),
            w!("other"),
            WS_OVERLAPPEDWINDOW,
            64,
            64,
        )?;
        let desc = description(window.handle, 64, 64);
        let dxgi = self.device.cast::<IDXGIDevice>()?;
        let adapter = unsafe { dxgi.GetAdapter() }?;
        let factory: IDXGIFactory = unsafe { adapter.GetParent() }?;
        let mut chain = None;
        unsafe { factory.CreateSwapChain(&self.device, &desc, &mut chain) }.ok()?;
        run(&chain.ok_or(E_POINTER)?);
        Ok(())
    }
}
