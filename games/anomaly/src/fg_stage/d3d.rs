// SPDX-License-Identifier: MIT
//! Borrowed engine/DXGI interfaces and owned references for one FG handoff.
use super::Raw;
use std::ptr::null_mut;
use windows::Win32::Graphics::Direct3D11::{ID3D11DepthStencilView, ID3D11Device, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::core::Interface;

pub(super) unsafe extern "C" fn immediate_context(chain: Raw) -> Raw {
    let device = unsafe {
        IDXGISwapChain::from_raw_borrowed(&chain)
            .and_then(|chain| chain.GetDevice::<ID3D11Device>().ok())
    };
    device
        .and_then(|device| unsafe { device.GetImmediateContext().ok() })
        .map_or(null_mut(), Interface::into_raw)
}

pub(super) unsafe extern "C" fn backbuffer(chain: Raw) -> Raw {
    unsafe {
        IDXGISwapChain::from_raw_borrowed(&chain)
            .and_then(|chain| chain.GetBuffer::<ID3D11Texture2D>(0).ok())
    }
    .map_or(null_mut(), Interface::into_raw)
}

pub(super) unsafe extern "C" fn depth_texture(view: Raw) -> Raw {
    unsafe {
        ID3D11DepthStencilView::from_raw_borrowed(&view).and_then(|view| view.GetResource().ok())
    }
    .and_then(|resource| resource.cast::<ID3D11Texture2D>().ok())
    .map_or(null_mut(), Interface::into_raw)
}

pub(super) unsafe extern "C" fn output_window(chain: Raw, window: *mut usize) -> bool {
    let Some(description) = (unsafe {
        IDXGISwapChain::from_raw_borrowed(&chain).and_then(|chain| chain.GetDesc().ok())
    }) else {
        return false;
    };
    unsafe { window.write(description.OutputWindow.0 as usize) };
    true
}
