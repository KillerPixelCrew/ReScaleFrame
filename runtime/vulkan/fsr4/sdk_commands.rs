// SPDX-License-Identifier: MIT
use super::{Dispatch, Handle, abi::*, dx12, rsf_core_fsr4sr_sdk_dispatch};
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

type Describe = unsafe extern "C" fn(Handle, u32, *mut FfxApiResource);

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_find_adapter(
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_create_device(
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

const ALLOCATORS: usize = 3;

#[repr(C)]
struct CommandObjects {
    completion: Handle,
    event: Handle,
    allocators: [Handle; ALLOCATORS],
    list: Handle,
}

const _: () = assert!(size_of::<CommandObjects>() == 6 * size_of::<Handle>());

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_create_commands(
    device: Handle,
    out: &mut CommandObjects,
) -> i32 {
    let device: &ID3D12Device = unsafe { dx12::borrow(&device) };
    let fence = unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) };
    let code = dx12::store_interface(fence, &mut out.completion);
    if code != 0 {
        return code;
    }
    let Ok(event) = (unsafe { CreateEventW(None, false, false, PCWSTR::null()) }) else {
        return -10;
    };
    out.event = event.0;
    let kind = D3D12_COMMAND_LIST_TYPE_DIRECT;
    for output in &mut out.allocators {
        let allocator = unsafe { device.CreateCommandAllocator::<ID3D12CommandAllocator>(kind) };
        let code = dx12::store_interface(allocator, output);
        if code != 0 {
            return code;
        }
    }
    let allocator = unsafe { dx12::borrow(&out.allocators[0]) };
    match dx12::create_list(device, allocator, 0, kind) {
        Ok(list) => {
            let result = unsafe { list.Close() };
            out.list = list.into_raw();
            dx12::status(result)
        }
        Err(error) => error.code().0,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_completed(fence: Handle) -> u64 {
    unsafe { dx12::borrow::<ID3D12Fence>(&fence).GetCompletedValue() }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_wait_event(
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_shared_fence(
    device: Handle,
    fence: &mut Handle,
    handle: &mut Handle,
) -> i32 {
    unsafe { dx12::create_shared_fence(device, fence, handle) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_release_image(image: Handle) {
    unsafe { dx12::release_image(image) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_describe(image: Handle, output: &mut super::super::Texture) {
    let desc = unsafe { dx12::borrow::<ID3D12Resource>(&image).GetDesc() };
    output.width = desc.Width;
    output.height = desc.Height;
    output.format = desc.Format.0 as u32;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_shared_image(
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_record(
    commands: Handle,
    allocator: Handle,
    frame: &Dispatch,
    context: *mut ffxContext,
    dispatch: PfnFfxDispatch,
    describe: Describe,
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

    let mut desc = ffxDispatchDescUpscale::default();
    unsafe {
        rsf_core_fsr4sr_sdk_dispatch(frame, commands, &mut desc);
        // FidelityFX owns the DXGI format and resource-description conventions.
        const COMPUTE_READ: u32 = FFX_API_RESOURCE_STATE_COMPUTE_READ as u32;
        const UNORDERED_ACCESS: u32 = FFX_API_RESOURCE_STATE_UNORDERED_ACCESS as u32;
        describe(frame.color, COMPUTE_READ, &mut desc.color);
        describe(frame.depth, COMPUTE_READ, &mut desc.depth);
        describe(frame.motion, COMPUTE_READ, &mut desc.motionVectors);
        describe(frame.output, UNORDERED_ACCESS, &mut desc.output);
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_execute(queue: Handle, commands: Handle) {
    unsafe { dx12::execute(queue, commands) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4sr_queue_fence(
    queue: Handle,
    fence: Handle,
    value: u64,
    signal: u32,
) -> i32 {
    unsafe { dx12::queue_fence(queue, fence, value, signal != 0) }
}
