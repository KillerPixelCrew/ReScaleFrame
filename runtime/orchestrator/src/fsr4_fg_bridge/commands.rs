// SPDX-License-Identifier: MIT
use super::native::graphics;
use super::{Handle, QueueDesc, SwapchainDesc, Texture, dx12};
use crate::d3d12_fence::wait_fence;
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

/// The capsule's graphics slots. The SDK retains the address of `chain`.
#[repr(C)]
pub(super) struct Graphics {
    pub(super) factory: Handle,
    pub(super) adapter: Handle,
    pub(super) device: Handle,
    pub(super) queue: Handle,
    pub(super) allocator: Handle,
    pub(super) commands: Handle,
    pub(super) completion: Handle,
    pub(super) shared: Handle,
    pub(super) chain: Handle,
    pub(super) event: Handle,
}

pub(super) unsafe extern "C" fn release_chain(native: Handle) {
    let graphics = unsafe { graphics(native) };
    if !graphics.chain.is_null() {
        drop(unsafe { IDXGISwapChain4::from_raw(graphics.chain) });
        graphics.chain = core::ptr::null_mut();
    }
}

pub(super) unsafe extern "C" fn create_factory(native: Handle, flags: u32) -> i32 {
    let graphics = unsafe { graphics(native) };
    dx12::store_interface(
        unsafe { CreateDXGIFactory2::<IDXGIFactory4>(DXGI_CREATE_FACTORY_FLAGS(flags)) },
        &mut graphics.factory,
    )
}

pub(super) unsafe extern "C" fn find_adapter(native: Handle, luid: u64) -> i32 {
    let graphics = unsafe { graphics(native) };
    let factory: &IDXGIFactory4 = unsafe { dx12::borrow(&graphics.factory) };
    let luid = LUID {
        LowPart: luid as u32,
        HighPart: (luid >> 32) as i32,
    };
    dx12::store_interface(
        unsafe { factory.EnumAdapterByLuid::<IDXGIAdapter1>(luid) },
        &mut graphics.adapter,
    )
}

pub(super) unsafe extern "C" fn get_adapter_vendor(native: Handle, vendor: *mut u32) -> i32 {
    let graphics = unsafe { graphics(native) };
    let adapter: &IDXGIAdapter1 = unsafe { dx12::borrow(&graphics.adapter) };
    dx12::status(unsafe { adapter.GetDesc1() }.map(|desc| unsafe { vendor.write(desc.VendorId) }))
}

pub(super) unsafe extern "C" fn create_device(native: Handle, level: u32) -> i32 {
    let graphics = unsafe { graphics(native) };
    unsafe {
        dx12::create_device(
            graphics.adapter,
            D3D_FEATURE_LEVEL(level as i32),
            &mut graphics.device,
        )
    }
}

pub(super) unsafe extern "C" fn create_event(native: Handle, manual: u32, initial: u32) -> i32 {
    let graphics = unsafe { graphics(native) };
    use windows_sys::Win32::{Foundation::GetLastError, System::Threading::CreateEventW};
    graphics.event = unsafe {
        CreateEventW(
            core::ptr::null(),
            i32::from(manual != 0),
            i32::from(initial != 0),
            core::ptr::null(),
        )
    };
    if graphics.event.is_null() {
        windows::core::HRESULT::from_win32(unsafe { GetLastError() }).0
    } else {
        0
    }
}

pub(super) unsafe extern "C" fn create_queue(native: Handle, desc: *const QueueDesc) -> i32 {
    let graphics = unsafe { graphics(native) };
    let desc = unsafe { &*desc };
    let device: &ID3D12Device = unsafe { dx12::borrow(&graphics.device) };
    let desc = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE(desc.list_type as i32),
        Priority: desc.priority,
        Flags: D3D12_COMMAND_QUEUE_FLAGS(desc.flags as i32),
        NodeMask: desc.node_mask,
    };
    dx12::store_interface(
        unsafe { device.CreateCommandQueue::<ID3D12CommandQueue>(&desc) },
        &mut graphics.queue,
    )
}

pub(super) unsafe extern "C" fn create_allocator(native: Handle, kind: u32) -> i32 {
    let graphics = unsafe { graphics(native) };
    let device: &ID3D12Device = unsafe { dx12::borrow(&graphics.device) };
    let kind = D3D12_COMMAND_LIST_TYPE(kind as i32);
    dx12::store_interface(
        unsafe { device.CreateCommandAllocator::<ID3D12CommandAllocator>(kind) },
        &mut graphics.allocator,
    )
}

pub(super) unsafe extern "C" fn create_command_list(
    native: Handle,
    node_mask: u32,
    kind: u32,
) -> i32 {
    let graphics = unsafe { graphics(native) };
    let device = unsafe { dx12::borrow(&graphics.device) };
    let allocator = unsafe { dx12::borrow(&graphics.allocator) };
    dx12::store_interface(
        dx12::create_list(
            device,
            allocator,
            node_mask,
            D3D12_COMMAND_LIST_TYPE(kind as i32),
        ),
        &mut graphics.commands,
    )
}

pub(super) unsafe extern "C" fn create_completion_fence(
    native: Handle,
    value: u64,
    flags: u32,
) -> i32 {
    let graphics = unsafe { graphics(native) };
    let device: &ID3D12Device = unsafe { dx12::borrow(&graphics.device) };
    let result =
        unsafe { device.CreateFence::<ID3D12Fence>(value, D3D12_FENCE_FLAGS(flags as i32)) };
    dx12::store_interface(result, &mut graphics.completion)
}

pub(super) unsafe extern "C" fn create_fence(native: Handle, handle: *mut Handle) -> i32 {
    let graphics = unsafe { graphics(native) };
    unsafe { dx12::create_shared_fence(graphics.device, &mut graphics.shared, &mut *handle) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn RsfFsr4FgReleaseImage(image: Handle) {
    unsafe { dx12::release_image(image) };
}

pub(super) unsafe extern "C" fn reset_commands(native: Handle) -> i32 {
    let graphics = unsafe { graphics(native) };
    dx12::status(unsafe {
        dx12::reset_commands(dx12::borrow(&graphics.commands), graphics.allocator)
    })
}

pub(super) unsafe extern "C" fn close_commands(native: Handle) -> i32 {
    let graphics = unsafe { graphics(native) };
    dx12::status(unsafe { dx12::borrow::<ID3D12GraphicsCommandList>(&graphics.commands).Close() })
}

const _: () = {
    assert!(size_of::<DXGI_SWAP_CHAIN_DESC1>() == 48);
    assert!(core::mem::offset_of!(DXGI_SWAP_CHAIN_DESC1, SampleDesc) == 16);
    assert!(core::mem::offset_of!(DXGI_SWAP_CHAIN_DESC1, SwapEffect) == 36);
};

pub(super) unsafe fn dxgi_description(
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

pub(super) unsafe extern "C" fn create_image(
    native: Handle,
    width: u32,
    height: u32,
    format: u32,
    handle: *mut Handle,
    resource: *mut Handle,
) -> i32 {
    let graphics = unsafe { graphics(native) };
    let device = unsafe { dx12::borrow(&graphics.device) };
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

pub(super) unsafe extern "C" fn describe_texture(resource: Handle, out: *mut Texture) {
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

pub(super) unsafe extern "C" fn shader_read(native: Handle, image: Handle, enter: u32) {
    let graphics = unsafe { graphics(native) };
    let commands = unsafe { dx12::borrow(&graphics.commands) };
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

pub(super) unsafe extern "C" fn copy_to_backbuffer(native: Handle, color: Handle) -> i32 {
    let graphics = unsafe { graphics(native) };
    let chain: &IDXGISwapChain4 = unsafe { dx12::borrow(&graphics.chain) };
    let backbuffer: ID3D12Resource =
        match unsafe { chain.GetBuffer(chain.GetCurrentBackBufferIndex()) } {
            Ok(buffer) => buffer,
            Err(error) => return error.code().0,
        };
    let common = D3D12_RESOURCE_STATE_COMMON.0 as u32;
    let present = D3D12_RESOURCE_STATE_PRESENT.0 as u32;
    unsafe {
        rsf_core_d3d12_copy_transitioned(
            graphics.commands,
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

pub(super) unsafe extern "C" fn execute(native: Handle) {
    let graphics = unsafe { graphics(native) };
    unsafe { dx12::execute(graphics.queue, graphics.commands) };
}

pub(super) unsafe extern "C" fn signal(native: Handle, value: u64, shared: u32) -> i32 {
    let graphics = unsafe { graphics(native) };
    let fence = if shared != 0 {
        graphics.shared
    } else {
        graphics.completion
    };
    unsafe { dx12::queue_fence(graphics.queue, fence, value, true) }
}

pub(super) unsafe extern "C" fn present(native: Handle, vsync: u32) -> i32 {
    let graphics = unsafe { graphics(native) };
    let chain: &IDXGISwapChain4 = unsafe { dx12::borrow(&graphics.chain) };
    dx12::status(unsafe { chain.Present(vsync, DXGI_PRESENT(0)) }.ok())
}

pub(super) unsafe extern "C" fn get_present_count(native: Handle, count: *mut u32) -> i32 {
    let graphics = unsafe { graphics(native) };
    let chain: &IDXGISwapChain4 = unsafe { dx12::borrow(&graphics.chain) };
    dx12::status(
        unsafe { chain.GetLastPresentCount() }.map(|presented| unsafe { count.write(presented) }),
    )
}

pub(super) unsafe extern "C" fn wait_shared(native: Handle, value: u64) -> i32 {
    let graphics = unsafe { graphics(native) };
    unsafe { dx12::queue_fence(graphics.queue, graphics.shared, value, false) }
}

pub(super) unsafe extern "C" fn wait_local(native: Handle, value: u64) -> i32 {
    let graphics = unsafe { graphics(native) };
    unsafe { wait_fence(graphics.completion, value, graphics.event, 10_000) }
}
