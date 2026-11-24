// SPDX-License-Identifier: MIT
//! Typed COM operations on the pipeline's leased graphics interfaces.
use super::Handle;
use crate::d3d11_d3d12::transition_barrier;
use std::ptr::null_mut;
use windows::Win32::{
    Foundation::HWND,
    Graphics::{
        Direct3D12::*,
        Dxgi::{
            Common::{DXGI_FORMAT, DXGI_FORMAT_R8G8B8A8_UNORM},
            *,
        },
    },
};
use windows::core::Interface;

pub(super) const STATE_COMMON: u32 = D3D12_RESOURCE_STATE_COMMON.0 as u32;
pub(super) const STATE_PRESENT: u32 = D3D12_RESOURCE_STATE_PRESENT.0 as u32;
pub(super) const STATE_RENDER_TARGET: u32 = D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32;
pub(super) const STATE_SHADER_INPUT: u32 = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0 as u32;
pub(super) const STATE_COPY_DEST: u32 = D3D12_RESOURCE_STATE_COPY_DEST.0 as u32;
pub(super) const STATE_COPY_SOURCE: u32 = D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32;
pub(super) const CHAIN_FORMAT: u32 = DXGI_FORMAT_R8G8B8A8_UNORM.0 as u32;

unsafe extern "C" {
    fn rsf_d3d11_present_create_for_hwnd(
        factory: Handle,
        queue: Handle,
        window: Handle,
        desc: *const DXGI_SWAP_CHAIN_DESC1,
        out: *mut Handle,
    ) -> i32;
}

pub(super) unsafe extern "C" fn chain_create(
    queue: Handle,
    window: Handle,
    buffers: u32,
    format: u32,
    out: *mut Handle,
) -> i32 {
    unsafe { out.write(null_mut()) };
    let result = (|| {
        let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory2>() }?;
        let factory = factory.cast::<IDXGIFactory>()?;
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: super::window::INITIAL_EXTENT,
            Height: super::window::INITIAL_EXTENT,
            Format: DXGI_FORMAT(format as i32),
            SampleDesc: Common::DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: buffers,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: Common::DXGI_ALPHA_MODE_IGNORE,
            ..Default::default()
        };
        let mut chain = null_mut();
        let result = unsafe {
            rsf_d3d11_present_create_for_hwnd(factory.as_raw(), queue, window, &desc, &mut chain)
        };
        // Adopt whatever the bridge returned before checking the HRESULT. This also
        // releases the original interface if QueryInterface for the chain fails.
        let chain = (!chain.is_null()).then(|| unsafe { IDXGISwapChain1::from_raw(chain) });
        windows::core::HRESULT(result).ok()?;
        let chain = chain.ok_or_else(|| {
            windows::core::Error::from_hresult(windows::Win32::Foundation::E_UNEXPECTED)
        })?;
        chain.cast::<IDXGISwapChain3>()
    })();
    match result {
        Ok(chain) => {
            unsafe { out.write(chain.into_raw()) };
            0
        }
        Err(error) => error.code().0,
    }
}

unsafe fn borrow<T: Interface>(raw: &Handle) -> &T {
    unsafe { T::from_raw_borrowed(raw) }.expect("live OpenGL pipeline interface")
}

pub(super) unsafe extern "C" fn rsf_core_gl_chain_associate(chain: Handle, window: Handle) {
    let chain = unsafe { borrow::<IDXGISwapChain3>(&chain) };
    if let Ok(factory) = unsafe { chain.GetParent::<IDXGIFactory2>() } {
        let _ = unsafe { factory.MakeWindowAssociation(HWND(window), DXGI_MWA_NO_ALT_ENTER) };
    }
}

pub(super) unsafe extern "C" fn rsf_core_gl_chain_resize(
    chain: Handle,
    buffers: u32,
    width: u32,
    height: u32,
    format: u32,
) -> i32 {
    let chain = unsafe { borrow::<IDXGISwapChain3>(&chain) };
    unsafe {
        chain.ResizeBuffers(
            buffers,
            width,
            height,
            DXGI_FORMAT(format as i32),
            DXGI_SWAP_CHAIN_FLAG(0),
        )
    }
    .map_or_else(|error| error.code().0, |()| 0)
}

pub(super) unsafe extern "C" fn rsf_core_gl_chain_back_buffer(chain: Handle) -> Handle {
    let chain = unsafe { borrow::<IDXGISwapChain3>(&chain) };
    unsafe { chain.GetBuffer::<ID3D12Resource>(chain.GetCurrentBackBufferIndex()) }
        .map_or(null_mut(), |value| value.into_raw())
}

pub(super) unsafe extern "C" fn rsf_core_gl_chain_present(chain: Handle, interval: u32) -> i32 {
    unsafe { borrow::<IDXGISwapChain3>(&chain).Present(interval, DXGI_PRESENT(0)) }.0
}

// The adapter's vendor ID and software flag, both zero when DXGI cannot identify it.
pub(super) unsafe fn rsf_core_gl_adapter_identity(device: Handle) -> (u32, u32) {
    crate::d3d12_host::adapter_identity(unsafe { borrow::<ID3D12Device>(&device) })
        .map_or((0, 0), |(vendor, software)| (vendor, u32::from(software)))
}

pub(super) unsafe extern "C" fn rsf_core_gl_transition(
    list: Handle,
    resource: Handle,
    from: u32,
    to: u32,
) {
    // This descriptor borrows the owning transaction's resource reference.
    let barrier = transition_barrier(
        unsafe { borrow::<ID3D12Resource>(&resource) },
        D3D12_RESOURCE_STATES(from as i32),
        D3D12_RESOURCE_STATES(to as i32),
    );
    unsafe { borrow::<ID3D12GraphicsCommandList>(&list).ResourceBarrier(&[barrier]) };
}

pub(super) unsafe extern "C" fn rsf_core_gl_copy_resource(
    list: Handle,
    destination: Handle,
    source: Handle,
) {
    unsafe {
        borrow::<ID3D12GraphicsCommandList>(&list).CopyResource(
            borrow::<ID3D12Resource>(&destination),
            borrow::<ID3D12Resource>(&source),
        );
    }
}
