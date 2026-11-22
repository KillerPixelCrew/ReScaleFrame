// SPDX-License-Identifier: MIT
//! Typed COM operations on the pipeline's leased graphics interfaces.
use super::Handle;
use std::mem::ManuallyDrop;
use std::ptr::null_mut;
use windows::Win32::{
    Foundation::HWND,
    Graphics::{
        Direct3D12::*,
        Dxgi::{Common::DXGI_FORMAT, *},
    },
};
use windows::core::Interface;

unsafe fn borrow<T: Interface>(raw: &Handle) -> &T {
    unsafe { T::from_raw_borrowed(raw) }.expect("live OpenGL pipeline interface")
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_gl_chain_associate(chain: Handle, window: Handle) {
    let chain = unsafe { borrow::<IDXGISwapChain3>(&chain) };
    if let Ok(factory) = unsafe { chain.GetParent::<IDXGIFactory2>() } {
        let _ = unsafe { factory.MakeWindowAssociation(HWND(window), DXGI_MWA_NO_ALT_ENTER) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_gl_chain_resize(
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_gl_chain_back_buffer(chain: Handle) -> Handle {
    let chain = unsafe { borrow::<IDXGISwapChain3>(&chain) };
    unsafe { chain.GetBuffer::<ID3D12Resource>(chain.GetCurrentBackBufferIndex()) }
        .map_or(null_mut(), |value| value.into_raw())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_gl_chain_present(chain: Handle, interval: u32) -> i32 {
    unsafe { borrow::<IDXGISwapChain3>(&chain).Present(interval, DXGI_PRESENT(0)) }.0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_gl_adapter_identity(
    device: Handle,
    vendor: *mut u32,
    software: *mut u32,
) -> i32 {
    let device = unsafe { borrow::<ID3D12Device>(&device) };
    let description = unsafe {
        CreateDXGIFactory1::<IDXGIFactory4>()
            .and_then(|factory| factory.EnumAdapterByLuid::<IDXGIAdapter1>(device.GetAdapterLuid()))
            .and_then(|adapter| adapter.GetDesc1())
    };
    let Ok(description) = description else {
        return 0;
    };
    unsafe {
        vendor.write(description.VendorId);
        software.write(u32::from(
            description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0,
        ));
    }
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_gl_transition(list: Handle, resource: Handle, from: u32, to: u32) {
    // This descriptor borrows the owning transaction's resource reference.
    let barrier = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: ManuallyDrop::new(Some(unsafe { ID3D12Resource::from_raw(resource) })),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: D3D12_RESOURCE_STATES(from as i32),
                StateAfter: D3D12_RESOURCE_STATES(to as i32),
            }),
        },
    };
    unsafe { borrow::<ID3D12GraphicsCommandList>(&list).ResourceBarrier(&[barrier]) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_gl_copy_resource(list: Handle, destination: Handle, source: Handle) {
    unsafe {
        borrow::<ID3D12GraphicsCommandList>(&list).CopyResource(
            borrow::<ID3D12Resource>(&destination),
            borrow::<ID3D12Resource>(&source),
        );
    }
}
