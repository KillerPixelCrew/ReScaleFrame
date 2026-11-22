// SPDX-License-Identifier: MIT
use super::{Handle, SwapchainDesc, Texture, dx12};
use crate::d3d12_host::rsf_core_d3d12_copy_transitioned;
use windows::Win32::Graphics::{
    Direct3D12::*,
    Dxgi::{
        Common::*, DXGI_PRESENT, DXGI_SCALING, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT, DXGI_USAGE,
        IDXGISwapChain4,
    },
};
use windows::core::{BOOL, Interface};

const _: () = {
    assert!(size_of::<DXGI_SWAP_CHAIN_DESC1>() == 48);
    assert!(core::mem::offset_of!(DXGI_SWAP_CHAIN_DESC1, SampleDesc) == 16);
    assert!(core::mem::offset_of!(DXGI_SWAP_CHAIN_DESC1, SwapEffect) == 36);
};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_dxgi_description(
    desc: &mut DXGI_SWAP_CHAIN_DESC1,
    setup: &SwapchainDesc,
    first_attempt: u32,
) {
    if first_attempt != 0 {
        desc.Stereo = BOOL(i32::from(setup.stereo != 0));
        desc.SampleDesc.Quality = setup.sample_quality;
        desc.Scaling = DXGI_SCALING(setup.scaling as i32);
        desc.AlphaMode = DXGI_ALPHA_MODE(setup.alpha_mode as i32);
        desc.Flags = setup.flags;
    }
    desc.Width = setup.width;
    desc.Height = setup.height;
    desc.Format = DXGI_FORMAT(setup.format as i32);
    desc.SampleDesc.Count = setup.sample_count;
    desc.BufferUsage = DXGI_USAGE(setup.buffer_usage);
    desc.BufferCount = setup.buffer_count;
    desc.SwapEffect = DXGI_SWAP_EFFECT(setup.swap_effect as i32);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_shared_image(
    device: Handle,
    width: u32,
    height: u32,
    format: u32,
    handle: *mut Handle,
    resource: *mut Handle,
) -> i32 {
    let device = unsafe { dx12::borrow(&device) };
    match dx12::create_shared_texture(
        device,
        width,
        height,
        DXGI_FORMAT(format as i32),
        D3D12_RESOURCE_FLAG_NONE,
    ) {
        Ok((shared, image)) => {
            unsafe {
                handle.write(shared);
                resource.write(image);
            }
            0
        }
        Err(error) => error.code().0,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_describe_texture(resource: Handle, out: *mut Texture) {
    let desc = unsafe { dx12::borrow::<ID3D12Resource>(&resource).GetDesc() };
    unsafe {
        out.write(Texture {
            width: desc.Width,
            height: desc.Height,
            format: desc.Format.0 as u32,
            dimension: desc.Dimension.0 as u32,
            layers: u32::from(desc.DepthOrArraySize),
            samples: desc.SampleDesc.Count,
        });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_shader_read(commands: Handle, image: Handle, enter: u32) {
    let commands = unsafe { dx12::borrow(&commands) };
    let image = unsafe { dx12::borrow(&image) };
    let common = D3D12_RESOURCE_STATE_COMMON;
    let read = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    let (before, after) = if enter != 0 {
        (common, read)
    } else {
        (read, common)
    };
    dx12::transition(commands, image, before, after);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_copy_to_backbuffer(
    commands: Handle,
    chain: Handle,
    color: Handle,
) -> i32 {
    let chain: &IDXGISwapChain4 = unsafe { dx12::borrow(&chain) };
    let backbuffer: ID3D12Resource =
        match unsafe { chain.GetBuffer(chain.GetCurrentBackBufferIndex()) } {
            Ok(buffer) => buffer,
            Err(error) => return error.code().0,
        };
    let common = D3D12_RESOURCE_STATE_COMMON.0 as u32;
    let present = D3D12_RESOURCE_STATE_PRESENT.0 as u32;
    unsafe {
        rsf_core_d3d12_copy_transitioned(
            commands,
            backbuffer.as_raw(),
            present,
            present,
            color,
            common,
            common,
        )
    };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_execute(queue: Handle, commands: Handle) {
    unsafe { dx12::execute(queue, commands) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_queue_fence(
    queue: Handle,
    fence: Handle,
    value: u64,
    signal: u32,
) -> i32 {
    unsafe { dx12::queue_fence(queue, fence, value, signal != 0) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_present_dxgi(chain: Handle, vsync: u32) -> i32 {
    let chain: &IDXGISwapChain4 = unsafe { dx12::borrow(&chain) };
    dx12::status(unsafe { chain.Present(vsync, DXGI_PRESENT(0)) }.ok())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_present_count(chain: Handle, count: *mut u32) -> i32 {
    let chain: &IDXGISwapChain4 = unsafe { dx12::borrow(&chain) };
    dx12::status(
        unsafe { chain.GetLastPresentCount() }.map(|presented| unsafe { count.write(presented) }),
    )
}
