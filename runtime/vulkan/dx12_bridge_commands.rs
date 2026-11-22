// SPDX-License-Identifier: MIT
//! Borrowed COM calls shared by the FidelityFX Vulkan bridge archives.
use core::{ffi::c_void, mem::ManuallyDrop};
use windows::{
    Win32::{
        Foundation::{E_POINTER, GENERIC_ALL},
        Graphics::{
            Direct3D::D3D_FEATURE_LEVEL,
            Direct3D12::*,
            Dxgi::{Common::*, IDXGIAdapter1},
        },
    },
    core::{Interface, PCWSTR, Result},
};

pub(super) type Handle = *mut c_void;

pub(super) unsafe fn borrow<T: Interface>(raw: &Handle) -> &T {
    unsafe { T::from_raw_borrowed(raw) }.expect("live bridge COM interface")
}

pub(super) fn status(result: Result<()>) -> i32 {
    result.map_or_else(|error| error.code().0, |()| 0)
}

pub(super) fn store_interface<T: Interface>(result: Result<T>, output: &mut Handle) -> i32 {
    status(result.map(|owned| *output = owned.into_raw()))
}

pub(super) unsafe fn create_device(
    adapter: Handle,
    feature_level: D3D_FEATURE_LEVEL,
    output: &mut Handle,
) -> i32 {
    let adapter: &IDXGIAdapter1 = unsafe { borrow(&adapter) };
    let mut device: Option<ID3D12Device> = None;
    let code = status(unsafe { D3D12CreateDevice(adapter, feature_level, &mut device) });
    if let Some(device) = device {
        *output = device.into_raw();
    }
    code
}

pub(super) fn create_list(
    device: &ID3D12Device,
    allocator: &ID3D12CommandAllocator,
    node_mask: u32,
    kind: D3D12_COMMAND_LIST_TYPE,
) -> Result<ID3D12GraphicsCommandList> {
    unsafe { device.CreateCommandList(node_mask, kind, allocator, None) }
}

pub(super) unsafe fn release_image(image: Handle) {
    if !image.is_null() {
        drop(unsafe { ID3D12Resource::from_raw(image) });
    }
}

pub(super) unsafe fn create_shared_fence(
    device: Handle,
    owned_fence: &mut Handle,
    handle: &mut Handle,
) -> i32 {
    let device: &ID3D12Device = unsafe { borrow(&device) };
    if owned_fence.is_null() {
        let result = unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_SHARED) };
        match result {
            Ok(fence) => *owned_fence = fence.into_raw(),
            Err(error) => return error.code().0,
        }
    }
    // The capsule retains this reference even if exporting its NT handle fails.
    let fence: &ID3D12Fence = unsafe { borrow(owned_fence) };
    status(
        unsafe { device.CreateSharedHandle(fence, None, GENERIC_ALL.0, PCWSTR::null()) }
            .map(|exported| *handle = exported.0),
    )
}

pub(super) unsafe fn reset_commands(
    list: &ID3D12GraphicsCommandList,
    allocator: Handle,
) -> Result<()> {
    let allocator: &ID3D12CommandAllocator = unsafe { borrow(&allocator) };
    unsafe { allocator.Reset()? };
    unsafe { list.Reset(allocator, None) }
}

pub(super) fn create_shared_texture(
    device: &ID3D12Device,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    flags: D3D12_RESOURCE_FLAGS,
) -> Result<(Handle, Handle)> {
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: u64::from(width),
        Height: height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Flags: flags,
        ..Default::default()
    };
    let mut image: Option<ID3D12Resource> = None;
    unsafe {
        device.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_SHARED,
            &desc,
            D3D12_RESOURCE_STATE_COMMON,
            None,
            &mut image,
        )?;
    }
    let image = image.ok_or(windows::core::Error::from_hresult(E_POINTER))?;
    let handle = unsafe { device.CreateSharedHandle(&image, None, GENERIC_ALL.0, PCWSTR::null()) }?;
    Ok((handle.0, image.into_raw()))
}

pub(super) fn transition(
    commands: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
    let barrier = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // The resource is borrowed for this call.
                pResource: ManuallyDrop::new(Some(unsafe {
                    ID3D12Resource::from_raw(resource.as_raw())
                })),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    };
    unsafe { commands.ResourceBarrier(&[barrier]) };
}

pub(super) unsafe fn execute(queue: Handle, list: Handle) {
    let queue: &ID3D12CommandQueue = unsafe { borrow(&queue) };
    let list = ManuallyDrop::new(Some(unsafe { ID3D12CommandList::from_raw(list) }));
    unsafe { queue.ExecuteCommandLists(core::slice::from_ref(&*list)) };
}

pub(super) unsafe fn queue_fence(queue: Handle, fence: Handle, value: u64, signal: bool) -> i32 {
    let queue: &ID3D12CommandQueue = unsafe { borrow(&queue) };
    let Some(fence) = (unsafe { ID3D12Fence::from_raw_borrowed(&fence) }) else {
        return E_POINTER.0;
    };
    status(unsafe {
        if signal {
            queue.Signal(fence, value)
        } else {
            queue.Wait(fence, value)
        }
    })
}
