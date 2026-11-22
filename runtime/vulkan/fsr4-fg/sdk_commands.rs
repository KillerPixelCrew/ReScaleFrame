// SPDX-License-Identifier: MIT
use super::{Handle, QueueDesc, SwapchainDesc, Texture, dx12};
use crate::d3d12_host::rsf_core_d3d12_copy_transitioned;
use windows::Win32::{
    Foundation::LUID,
    Graphics::{
        Direct3D::D3D_FEATURE_LEVEL,
        Direct3D12::*,
        Dxgi::{
            Common::*, CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SCALING,
            DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT, DXGI_USAGE, IDXGIAdapter1, IDXGIFactory4,
            IDXGISwapChain4,
        },
    },
};
use windows::core::{BOOL, Interface};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_release_chain(chain: Handle) {
    if !chain.is_null() {
        drop(unsafe { IDXGISwapChain4::from_raw(chain) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_create_factory(flags: u32, out: &mut Handle) -> i32 {
    dx12::store_interface(
        unsafe { CreateDXGIFactory2::<IDXGIFactory4>(DXGI_CREATE_FACTORY_FLAGS(flags)) },
        out,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_find_adapter(
    factory: Handle,
    luid: u64,
    out: &mut Handle,
) -> i32 {
    let factory: &IDXGIFactory4 = unsafe { dx12::borrow(&factory) };
    let luid = LUID {
        LowPart: luid as u32,
        HighPart: (luid >> 32) as i32,
    };
    dx12::store_interface(
        unsafe { factory.EnumAdapterByLuid::<IDXGIAdapter1>(luid) },
        out,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_get_adapter_vendor(adapter: Handle, vendor: &mut u32) -> i32 {
    let adapter: &IDXGIAdapter1 = unsafe { dx12::borrow(&adapter) };
    dx12::status(unsafe { adapter.GetDesc1() }.map(|desc| *vendor = desc.VendorId))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_create_device(
    adapter: Handle,
    level: u32,
    out: &mut Handle,
) -> i32 {
    unsafe { dx12::create_device(adapter, D3D_FEATURE_LEVEL(level as i32), out) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_create_event(
    manual: u32,
    initial: u32,
    out: &mut Handle,
) -> i32 {
    use windows_sys::Win32::{Foundation::GetLastError, System::Threading::CreateEventW};
    *out = unsafe {
        CreateEventW(
            core::ptr::null(),
            i32::from(manual != 0),
            i32::from(initial != 0),
            core::ptr::null(),
        )
    };
    if out.is_null() {
        windows::core::HRESULT::from_win32(unsafe { GetLastError() }).0
    } else {
        0
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_create_queue(
    device: Handle,
    desc: &QueueDesc,
    out: &mut Handle,
) -> i32 {
    let device: &ID3D12Device = unsafe { dx12::borrow(&device) };
    let desc = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE(desc.list_type as i32),
        Priority: desc.priority,
        Flags: D3D12_COMMAND_QUEUE_FLAGS(desc.flags as i32),
        NodeMask: desc.node_mask,
    };
    dx12::store_interface(
        unsafe { device.CreateCommandQueue::<ID3D12CommandQueue>(&desc) },
        out,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_create_allocator(
    device: Handle,
    kind: u32,
    out: &mut Handle,
) -> i32 {
    let device: &ID3D12Device = unsafe { dx12::borrow(&device) };
    let kind = D3D12_COMMAND_LIST_TYPE(kind as i32);
    dx12::store_interface(
        unsafe { device.CreateCommandAllocator::<ID3D12CommandAllocator>(kind) },
        out,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_create_list(
    device: Handle,
    allocator: Handle,
    node_mask: u32,
    kind: u32,
    out: &mut Handle,
) -> i32 {
    let device = unsafe { dx12::borrow(&device) };
    let allocator = unsafe { dx12::borrow(&allocator) };
    dx12::store_interface(
        dx12::create_list(
            device,
            allocator,
            node_mask,
            D3D12_COMMAND_LIST_TYPE(kind as i32),
        ),
        out,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_create_completion_fence(
    device: Handle,
    value: u64,
    flags: u32,
    out: &mut Handle,
) -> i32 {
    let device: &ID3D12Device = unsafe { dx12::borrow(&device) };
    let result =
        unsafe { device.CreateFence::<ID3D12Fence>(value, D3D12_FENCE_FLAGS(flags as i32)) };
    dx12::store_interface(result, out)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_shared_fence(
    device: Handle,
    fence: &mut Handle,
    handle: &mut Handle,
) -> i32 {
    unsafe { dx12::create_shared_fence(device, fence, handle) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn RsfFsr4FgReleaseImage(image: Handle) {
    unsafe { dx12::release_image(image) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_reset_commands(commands: Handle, allocator: Handle) -> i32 {
    dx12::status(unsafe { dx12::reset_commands(dx12::borrow(&commands), allocator) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4fg_close_commands(commands: Handle) -> i32 {
    dx12::status(unsafe { dx12::borrow::<ID3D12GraphicsCommandList>(&commands).Close() })
}

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
