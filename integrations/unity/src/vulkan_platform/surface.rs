// SPDX-License-Identifier: MIT
use super::{DeviceHandles, borrow, release};
use std::ffi::c_void;
use std::ptr::null_mut;
use windows::Win32::{
    Foundation::HWND,
    Graphics::{
        Direct3D12::{ID3D12CommandQueue, ID3D12Resource},
        Dxgi::{Common::*, *},
    },
};
use windows::core::Interface;

const FORMAT: DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM;
const BUFFER_COUNT: u32 = 3;

pub(crate) unsafe extern "C" fn rsf_uvk_gpu_chain_create(
    shared: *const DeviceHandles,
    window: *mut c_void,
    width: u32,
    height: u32,
) -> *mut c_void {
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: FORMAT,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferCount: BUFFER_COUNT,
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        ..Default::default()
    };
    let chain = unsafe {
        let queue = borrow::<ID3D12CommandQueue>(&(*shared).queue);
        CreateDXGIFactory1::<IDXGIFactory2>()
            .and_then(|factory| {
                factory.CreateSwapChainForHwnd(
                    queue,
                    HWND(window),
                    &desc,
                    None,
                    None::<&IDXGIOutput>,
                )
            })
            .and_then(|chain| chain.cast::<IDXGISwapChain3>())
    };
    chain.map_or(null_mut(), Interface::into_raw)
}

pub(crate) unsafe extern "C" fn rsf_uvk_gpu_chain_extent(
    chain: *mut c_void,
    width: *mut u32,
    height: *mut u32,
) {
    let desc = unsafe { borrow::<IDXGISwapChain3>(&chain).GetDesc1() }.unwrap_or_default();
    unsafe {
        width.write(desc.Width);
        height.write(desc.Height);
    }
}

pub(crate) unsafe extern "C" fn rsf_uvk_gpu_chain_resize(
    chain: *mut c_void,
    width: u32,
    height: u32,
) -> bool {
    unsafe {
        borrow::<IDXGISwapChain3>(&chain)
            .ResizeBuffers(BUFFER_COUNT, width, height, FORMAT, DXGI_SWAP_CHAIN_FLAG(0))
            .is_ok()
    }
}

pub(crate) unsafe extern "C" fn rsf_uvk_gpu_chain_destroy(chain: *mut c_void) {
    unsafe { release::<IDXGISwapChain3>(chain) };
}

pub(crate) unsafe extern "C" fn rsf_uvk_gpu_chain_buffer(chain: *mut c_void) -> *mut c_void {
    let chain = unsafe { borrow::<IDXGISwapChain3>(&chain) };
    unsafe { chain.GetBuffer::<ID3D12Resource>(chain.GetCurrentBackBufferIndex()) }
        .map_or(null_mut(), Interface::into_raw)
}

pub(crate) unsafe extern "C" fn rsf_uvk_gpu_chain_present(chain: *mut c_void) -> bool {
    unsafe {
        borrow::<IDXGISwapChain3>(&chain)
            .Present(0, DXGI_PRESENT(0))
            .is_ok()
    }
}
