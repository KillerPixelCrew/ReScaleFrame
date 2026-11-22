// SPDX-License-Identifier: MIT
use super::{Dispatch, Handle, Texture, dx12, sdk};
use crate::ffx_abi::{FFX_API_RESOURCE_STATE_COMPUTE_READ, FFX_API_RESOURCE_STATE_UNORDERED_ACCESS, PfnFfxDispatch, ffxContext};
use crate::ffx_resource;
use windows::{
    Win32::{
        Foundation::{HANDLE, WAIT_OBJECT_0},
        Graphics::{
            Direct3D::D3D_FEATURE_LEVEL_12_0,
            Direct3D12::*,
            Dxgi::{
                Common::*, CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_ERROR_NOT_FOUND,
                IDXGIFactory4,
            },
        },
        System::Threading::{CreateEventW, WaitForSingleObject},
    },
    core::{Interface, PCWSTR},
};

pub(super) fn find_adapter(
    luid: u64,
    factory_out: &mut Handle,
    adapter_out: &mut Handle,
    vendor: &mut u32,
) -> i32 {
    let factory = match unsafe { CreateDXGIFactory2::<IDXGIFactory4>(DXGI_CREATE_FACTORY_FLAGS(0)) }
    {
        Ok(factory) => factory,
        Err(error) => return error.code().0,
    };
    *factory_out = factory.into_raw();
    let factory: &IDXGIFactory4 = unsafe { dx12::borrow(factory_out) };
    let mut index = 0_u32;
    loop {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => return -5,
            Err(error) => return error.code().0,
        };
        let description = match unsafe { adapter.GetDesc1() } {
            Ok(description) => description,
            Err(error) => return error.code().0,
        };
        let id = (u64::from(description.AdapterLuid.HighPart as u32) << 32)
            | u64::from(description.AdapterLuid.LowPart);
        if id == luid {
            *adapter_out = adapter.into_raw();
            *vendor = description.VendorId;
            return 0;
        }
        index = index.wrapping_add(1);
    }
}

pub(super) unsafe fn create_device(
    adapter: Handle,
    device_out: &mut Handle,
    queue_out: &mut Handle,
) -> i32 {
    let code = unsafe { dx12::create_device(adapter, D3D_FEATURE_LEVEL_12_0, device_out) };
    if code != 0 {
        return code;
    }
    let device: &ID3D12Device = unsafe { dx12::borrow(device_out) };
    let queue = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
        ..Default::default()
    };
    dx12::store_interface(
        unsafe { device.CreateCommandQueue::<ID3D12CommandQueue>(&queue) },
        queue_out,
    )
}

/// Creates the local completion fence, its event (-10 on failure), the allocators and a
/// closed list. Partial results stay with the caller's outputs.
pub(super) unsafe fn create_commands(
    device: Handle,
    completion: &mut Handle,
    event_out: &mut Handle,
    allocators: &mut [Handle],
    list_out: &mut Handle,
) -> i32 {
    let device: &ID3D12Device = unsafe { dx12::borrow(&device) };
    let fence = unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) };
    let code = dx12::store_interface(fence, completion);
    if code != 0 {
        return code;
    }
    let Ok(event) = (unsafe { CreateEventW(None, false, false, PCWSTR::null()) }) else {
        return -10;
    };
    *event_out = event.0;
    let kind = D3D12_COMMAND_LIST_TYPE_DIRECT;
    for output in allocators.iter_mut() {
        let allocator = unsafe { device.CreateCommandAllocator::<ID3D12CommandAllocator>(kind) };
        let code = dx12::store_interface(allocator, output);
        if code != 0 {
            return code;
        }
    }
    let allocator = unsafe { dx12::borrow(&allocators[0]) };
    match dx12::create_list(device, allocator, 0, kind) {
        Ok(list) => {
            let result = unsafe { list.Close() };
            *list_out = list.into_raw();
            dx12::status(result)
        }
        Err(error) => error.code().0,
    }
}

pub(super) unsafe fn completed(fence: Handle) -> u64 {
    unsafe { dx12::borrow::<ID3D12Fence>(&fence).GetCompletedValue() }
}

pub(super) unsafe fn wait_event(
    fence: Handle,
    event: Handle,
    value: u64,
    milliseconds: u32,
) -> i32 {
    let fence: &ID3D12Fence = unsafe { dx12::borrow(&fence) };
    let event = HANDLE(event);
    if let Err(error) = unsafe { fence.SetEventOnCompletion(value, event) } {
        return error.code().0;
    }
    if unsafe { WaitForSingleObject(event, milliseconds) } == WAIT_OBJECT_0 {
        0
    } else {
        -2
    }
}

pub(super) unsafe fn shared_fence(
    device: Handle,
    fence: &mut Handle,
    handle: &mut Handle,
) -> i32 {
    unsafe { dx12::create_shared_fence(device, fence, handle) }
}

pub(super) unsafe fn release_image(image: Handle) {
    unsafe { dx12::release_image(image) };
}

pub(super) unsafe fn describe(image: Handle) -> Texture {
    let desc = unsafe { dx12::borrow::<ID3D12Resource>(&image).GetDesc() };
    Texture {
        width: desc.Width,
        height: desc.Height,
        format: desc.Format.0 as u32,
    }
}

pub(super) unsafe fn shared_image(
    device: Handle,
    width: u32,
    height: u32,
    vk_format: u32,
    writable: u32,
    handle: *mut Handle,
    resource: *mut Handle,
) -> i32 {
    let (format, mut flags) = match vk_format {
        97 => (DXGI_FORMAT_R16G16B16A16_FLOAT, D3D12_RESOURCE_FLAG_NONE),
        83 => (DXGI_FORMAT_R16G16_FLOAT, D3D12_RESOURCE_FLAG_NONE),
        100 => (DXGI_FORMAT_R32_FLOAT, D3D12_RESOURCE_FLAG_NONE),
        126 => (
            DXGI_FORMAT_D32_FLOAT,
            D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
        ),
        _ => return -2,
    };
    if writable != 0 {
        if flags != D3D12_RESOURCE_FLAG_NONE {
            return -3;
        }
        flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    }
    let device = unsafe { dx12::borrow(&device) };
    match dx12::create_shared_texture(device, width, height, format, flags) {
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

pub(super) unsafe fn record(
    commands: Handle,
    allocator: Handle,
    frame: &Dispatch,
    context: *mut ffxContext,
    dispatch: PfnFfxDispatch,
) -> i32 {
    let Some(dispatch) = dispatch else { return -1 };
    let list: &ID3D12GraphicsCommandList = unsafe { dx12::borrow(&commands) };
    if let Err(error) = unsafe { dx12::reset_commands(list, allocator) } {
        return error.code().0;
    }
    const COMMON: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_COMMON;
    const READ: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    const WRITE: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
    let resources = [
        (frame.color, READ),
        (frame.depth, READ),
        (frame.motion, READ),
        (frame.output, WRITE),
    ];
    for (image, state) in &resources {
        dx12::transition(list, unsafe { dx12::borrow(image) }, COMMON, *state);
    }

    let mut desc = sdk::dispatch(frame, commands);
    // FidelityFX owns the DXGI format and resource-description conventions.
    const COMPUTE_READ: u32 = FFX_API_RESOURCE_STATE_COMPUTE_READ as u32;
    const UNORDERED_ACCESS: u32 = FFX_API_RESOURCE_STATE_UNORDERED_ACCESS as u32;
    unsafe {
        desc.color = ffx_resource::describe(frame.color, COMPUTE_READ);
        desc.depth = ffx_resource::describe(frame.depth, COMPUTE_READ);
        desc.motionVectors = ffx_resource::describe(frame.motion, COMPUTE_READ);
        desc.output = ffx_resource::describe(frame.output, UNORDERED_ACCESS);
    }
    let code = unsafe { dispatch(context, &raw const desc.header) } as i32;
    if code != 0 {
        let _ = unsafe { list.Close() };
        return code;
    }
    for (image, state) in &resources {
        dx12::transition(list, unsafe { dx12::borrow(image) }, *state, COMMON);
    }
    dx12::status(unsafe { list.Close() })
}

pub(super) unsafe fn execute(queue: Handle, commands: Handle) {
    unsafe { dx12::execute(queue, commands) };
}

pub(super) unsafe fn queue_fence(queue: Handle, fence: Handle, value: u64, signal: bool) -> i32 {
    unsafe { dx12::queue_fence(queue, fence, value, signal) }
}
