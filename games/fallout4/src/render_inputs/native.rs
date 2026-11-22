// SPDX-License-Identifier: MIT
//! Typed D3D11 input observations and references transferred to the caller's lease.

use super::{DeviceFacts, Lease, ResourceFacts, SwapFacts, ViewportFacts};
use crate::d3d11::{identity, viewport as read_viewport};
use std::ffi::c_void;
use std::ptr;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_DEVICE_CONTEXT_IMMEDIATE, D3D11_RENDER_TARGET_VIEW_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC,
    D3D11_TEXTURE2D_DESC, ID3D11Device, ID3D11DeviceContext, ID3D11RenderTargetView,
    ID3D11ShaderResourceView, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::core::Interface;

/// Raw owned references for the C++ World caller. Each nonnull field holds one reference.
/// The caller's guard releases any values it has not attached to its output.
#[repr(C)]
pub(super) struct NativeLease {
    context: *mut c_void,
    chain: *mut c_void,
    textures: [*mut c_void; 6],
}

const _: () = assert!(size_of::<NativeLease>() == 8 * size_of::<usize>());

pub(super) fn owner(lease: Lease, context: usize, chain: usize) -> DeviceFacts {
    let context = context as *mut c_void;
    let chain = chain as *mut c_void;
    let mut facts = DeviceFacts::default();
    // SAFETY: the owner snapshot verified these native fields, and the caller
    // supplies a fresh lease on its stack.
    let (Some(context), Some(chain)) = (
        unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) },
        unsafe { IDXGISwapChain::from_raw_borrowed(&chain) },
    ) else {
        return facts;
    };
    unsafe {
        (*lease).context = context.clone().into_raw();
        (*lease).chain = chain.clone().into_raw();
    }
    facts.immediate = u32::from(unsafe { context.GetType() } == D3D11_DEVICE_CONTEXT_IMMEDIATE);
    facts.device = unsafe { context.GetDevice() }.as_ref().map_or(0, identity);
    if let Ok(device) = unsafe { chain.GetDevice::<ID3D11Device>() } {
        facts.chain_device_read = 1;
        facts.chain_device = identity(&device);
    }
    facts
}

pub(super) fn viewport(context: usize) -> ViewportFacts {
    let raw = context as *mut c_void;
    // SAFETY: the owner snapshot names a live device context.
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }) else {
        return ViewportFacts::default();
    };
    let (count, copied, viewport) = read_viewport(context);
    ViewportFacts {
        count,
        copied,
        viewport,
    }
}

pub(super) fn swap(chain: usize) -> SwapFacts {
    let raw = chain as *mut c_void;
    // SAFETY: the owner snapshot and the current lease name a live swap chain.
    let Some(chain) = (unsafe { IDXGISwapChain::from_raw_borrowed(&raw) }) else {
        return SwapFacts::default();
    };
    match unsafe { chain.GetDesc() } {
        Ok(desc) => SwapFacts {
            read: 1,
            window: desc.OutputWindow.0 as u64,
            width: desc.BufferDesc.Width,
            height: desc.BufferDesc.Height,
            ..SwapFacts::default()
        },
        Err(_) => SwapFacts::default(),
    }
}

pub(super) fn resource(lease: Lease, slot: u32, key: u64, render_target: bool) -> ResourceFacts {
    let mut facts = ResourceFacts::default();
    let raw = key as *mut c_void;
    // SAFETY: the native slot supplies a live view matching render_target.
    let viewed = if render_target {
        let Some(view) = (unsafe { ID3D11RenderTargetView::from_raw_borrowed(&raw) }) else {
            return facts;
        };
        let mut desc = D3D11_RENDER_TARGET_VIEW_DESC::default();
        unsafe { view.GetDesc(&mut desc) };
        facts.view_format = desc.Format.0 as u32;
        facts.view_dimension = desc.ViewDimension.0 as u32;
        facts.view_mip = unsafe { desc.Anonymous.Texture2D.MipSlice };
        unsafe { view.GetResource() }
    } else {
        let Some(view) = (unsafe { ID3D11ShaderResourceView::from_raw_borrowed(&raw) }) else {
            return facts;
        };
        let mut desc = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
        unsafe { view.GetDesc(&mut desc) };
        let texture = unsafe { desc.Anonymous.Texture2D };
        facts.view_format = desc.Format.0 as u32;
        facts.view_dimension = desc.ViewDimension.0 as u32;
        facts.view_mip = texture.MostDetailedMip;
        facts.view_mips = texture.MipLevels;
        unsafe { view.GetResource() }
    };
    facts.read = 1;
    let Ok(viewed) = viewed else { return facts };
    facts.identity = identity(&viewed);
    let Ok(texture) = viewed.cast::<ID3D11Texture2D>() else {
        return facts;
    };
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    facts.pointer = texture.as_raw() as u64;
    facts.width = desc.Width;
    facts.height = desc.Height;
    facts.format = desc.Format.0 as u32;
    facts.mip_levels = desc.MipLevels;
    facts.array_size = desc.ArraySize;
    facts.sample_count = desc.SampleDesc.Count;
    facts.sample_quality = desc.SampleDesc.Quality;
    facts.bind_flags = desc.BindFlags;
    facts.device = unsafe { texture.GetDevice() }.as_ref().map_or(0, identity);
    // SAFETY: the caller supplies its writable stack lease. NO_SLOT is outside the array.
    if let Some(owned) = unsafe { &mut (*lease).textures }.get_mut(slot as usize) {
        release_texture(std::mem::replace(owned, texture.into_raw()));
    }
    facts
}

pub(super) fn release(lease: Lease, slot: u32) {
    // SAFETY: the caller supplies its writable stack lease.
    if let Some(owned) = unsafe { &mut (*lease).textures }.get_mut(slot as usize) {
        release_texture(std::mem::replace(owned, ptr::null_mut()));
    }
}

fn release_texture(raw: *mut c_void) {
    if !raw.is_null() {
        // SAFETY: the nonnull raw value carries the reference removed from the lease.
        drop(unsafe { ID3D11Texture2D::from_raw(raw) });
    }
}
