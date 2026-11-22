// SPDX-License-Identifier: MIT
//! Typed DXGI and D3D12 calls for the GL interop owner's native table.
use super::Handle;
// The leaves gl_shared_surface.rs calls as `native::*`; its tests fake the same names.
pub(super) use super::native_gl::{
    acquire, current as gl_current, error as gl_error, extension_at as gl_extension_at,
    extension_count as gl_extension_count, finish, lookup as gl_lookup, luid as gl_luid,
    memory_delete, memory_import, release, semaphore_delete, semaphore_import, texture_delete,
    texture_store,
};
use std::mem::ManuallyDrop;
use std::ptr::{null, null_mut};
use windows::Win32::{
    Foundation::{E_FAIL, GENERIC_ALL, LUID},
    Graphics::{
        Direct3D::D3D_FEATURE_LEVEL,
        Direct3D12::*,
        Dxgi::{Common::*, CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory4},
    },
};
use windows::core::{Interface, PCWSTR, Result};
use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::CreateEventW};

unsafe fn borrow<T: Interface>(raw: &Handle) -> &T {
    unsafe { T::from_raw_borrowed(raw) }.expect("live GL interop interface")
}

fn status(result: Result<()>) -> i32 {
    result.map_or_else(|error| error.code().0, |()| 0)
}

unsafe fn publish<T: Interface>(result: Result<T>, out: *mut Handle) -> i32 {
    unsafe { out.write(null_mut()) };
    match result {
        Ok(value) => {
            unsafe { out.write(value.into_raw()) };
            0
        }
        Err(error) => error.code().0,
    }
}

pub(super) unsafe extern "C" fn factory_create(out: *mut Handle) -> i32 {
    unsafe { publish(CreateDXGIFactory1::<IDXGIFactory4>(), out) }
}

pub(super) unsafe extern "C" fn adapter_find(
    factory: Handle,
    luid: *const u8,
    out: *mut Handle,
) -> i32 {
    let factory = unsafe { borrow::<IDXGIFactory4>(&factory) };
    let luid = unsafe { luid.cast::<LUID>().read_unaligned() };
    unsafe { publish(factory.EnumAdapterByLuid::<IDXGIAdapter1>(luid), out) }
}

pub(super) unsafe extern "C" fn device_create(
    adapter: Handle,
    level: u32,
    out: *mut Handle,
) -> i32 {
    let adapter = unsafe { borrow::<IDXGIAdapter1>(&adapter) };
    let mut device = None::<ID3D12Device>;
    let result =
        unsafe { D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL(level as i32), &mut device) };
    unsafe {
        publish(
            result.and_then(|()| device.ok_or_else(|| E_FAIL.into())),
            out,
        )
    }
}

pub(super) unsafe extern "C" fn queue_create(device: Handle, out: *mut Handle) -> i32 {
    let device = unsafe { borrow::<ID3D12Device>(&device) };
    let desc = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
        ..Default::default()
    };
    unsafe { publish(device.CreateCommandQueue::<ID3D12CommandQueue>(&desc), out) }
}

pub(super) unsafe extern "C" fn device_luid(device: Handle, out: *mut u8) {
    let device = unsafe { borrow::<ID3D12Device>(&device) };
    unsafe { out.cast::<LUID>().write_unaligned(device.GetAdapterLuid()) };
}

pub(super) unsafe extern "C" fn queue_device(queue: Handle, out: *mut Handle) -> i32 {
    let queue = unsafe { borrow::<ID3D12CommandQueue>(&queue) };
    let mut device = None::<ID3D12Device>;
    let result = unsafe { queue.GetDevice(&mut device) };
    unsafe {
        publish(
            result.and_then(|()| device.ok_or_else(|| E_FAIL.into())),
            out,
        )
    }
}

pub(super) unsafe extern "C" fn fence_create(device: Handle, out: *mut Handle) -> i32 {
    let device = unsafe { borrow::<ID3D12Device>(&device) };
    unsafe {
        publish(
            device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_SHARED),
            out,
        )
    }
}

pub(super) unsafe extern "C" fn shared_handle(
    device: Handle,
    object: Handle,
    out: *mut Handle,
) -> i32 {
    let device = unsafe { borrow::<ID3D12Device>(&device) };
    let object = unsafe { borrow::<ID3D12DeviceChild>(&object) };
    unsafe { out.write(null_mut()) };
    let created = unsafe { device.CreateSharedHandle(object, None, GENERIC_ALL.0, PCWSTR::null()) };
    status(created.map(|handle| unsafe { out.write(handle.0) }))
}

pub(super) unsafe extern "C" fn event_create() -> Handle {
    unsafe { CreateEventW(null(), 0, 0, null()) }
}

pub(super) unsafe extern "C" fn handle_close(handle: Handle) {
    unsafe { CloseHandle(handle) };
}

pub(super) unsafe extern "C" fn slot_create(
    device: Handle,
    allocator: *mut Handle,
    list: *mut Handle,
) -> i32 {
    let device = unsafe { borrow::<ID3D12Device>(&device) };
    unsafe {
        allocator.write(null_mut());
        list.write(null_mut());
    }
    status((|| unsafe {
        let commands = device
            .CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
        let recorded: ID3D12GraphicsCommandList = device.CreateCommandList(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            &commands,
            None::<&ID3D12PipelineState>,
        )?;
        recorded.Close()?;
        allocator.write(commands.into_raw());
        list.write(recorded.into_raw());
        Ok(())
    })())
}

pub(super) unsafe extern "C" fn slot_reset(allocator: Handle, list: Handle) -> i32 {
    let allocator = unsafe { borrow::<ID3D12CommandAllocator>(&allocator) };
    let list = unsafe { borrow::<ID3D12GraphicsCommandList>(&list) };
    status(unsafe {
        allocator
            .Reset()
            .and_then(|()| list.Reset(allocator, None::<&ID3D12PipelineState>))
    })
}

pub(super) unsafe extern "C" fn list_close(list: Handle) -> i32 {
    status(unsafe { borrow::<ID3D12GraphicsCommandList>(&list).Close() })
}

pub(super) unsafe extern "C" fn fence_completed(fence: Handle) -> u64 {
    unsafe { borrow::<ID3D12Fence>(&fence).GetCompletedValue() }
}

pub(super) unsafe extern "C" fn fence_wait(
    fence: Handle,
    value: u64,
    event: Handle,
    timeout: u32,
) -> i32 {
    i32::from(unsafe { crate::d3d12_fence::wait_fence(fence, value, event, timeout) } >= 0)
}

pub(super) unsafe extern "C" fn queue_wait(queue: Handle, fence: Handle, value: u64) -> i32 {
    status(unsafe {
        borrow::<ID3D12CommandQueue>(&queue).Wait(borrow::<ID3D12Fence>(&fence), value)
    })
}

pub(super) unsafe extern "C" fn queue_execute(queue: Handle, list: Handle) {
    let queue = unsafe { borrow::<ID3D12CommandQueue>(&queue) };
    // The recording slot already owns this reference.
    let list = ManuallyDrop::new(Some(unsafe { ID3D12CommandList::from_raw(list) }));
    unsafe { queue.ExecuteCommandLists(std::slice::from_ref(&*list)) };
}

pub(super) unsafe extern "C" fn queue_signal(queue: Handle, fence: Handle, value: u64) -> i32 {
    status(unsafe {
        borrow::<ID3D12CommandQueue>(&queue).Signal(borrow::<ID3D12Fence>(&fence), value)
    })
}

pub(super) unsafe extern "C" fn resource_create(
    device: Handle,
    width: u32,
    height: u32,
    format: u32,
    out: *mut Handle,
) -> i32 {
    let device = unsafe { borrow::<ID3D12Device>(&device) };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: u64::from(width),
        Height: height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT(format as i32),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET
            | D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS
            | D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS,
        ..Default::default()
    };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        CreationNodeMask: 1,
        VisibleNodeMask: 1,
        ..Default::default()
    };
    let mut resource = None::<ID3D12Resource>;
    let result = unsafe {
        device.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_SHARED,
            &desc,
            D3D12_RESOURCE_STATE_COMMON,
            None,
            &mut resource,
        )
    };
    unsafe {
        publish(
            result.and_then(|()| resource.ok_or_else(|| E_FAIL.into())),
            out,
        )
    }
}
