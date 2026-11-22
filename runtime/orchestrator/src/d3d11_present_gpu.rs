// SPDX-License-Identifier: MIT
//! D3D12 platform leaves with the same raw output and HRESULT contracts used by
//! the fake platform and the C++ state capsule.
use super::{E_FAIL, S_OK};
use crate::com_ref::ComRef;
use crate::d3d11_present_abi::Raw;
use std::mem::ManuallyDrop;
use std::ptr::{null, null_mut};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::core::{IUnknown, Interface};
use windows_sys::Win32::Foundation::{CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle};
use windows_sys::Win32::System::Threading::{CreateEventW, GetCurrentProcess, SetEvent};

unsafe fn borrow<T: Interface>(value: &Raw) -> &T {
    unsafe { T::from_raw_borrowed(value) }.unwrap()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_queue(device: Raw, out: *mut Raw) -> i32 {
    let bound = unsafe { borrow::<ID3D12Device>(&device) };
    let desc = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
        ..Default::default()
    };
    unsafe {
        out.write(null_mut());
        (bound.vtable().CreateCommandQueue)(device, &desc, &ID3D12CommandQueue::IID, out)
    }
    .0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_fence(device: Raw, out: *mut Raw) -> i32 {
    let bound = unsafe { borrow::<ID3D12Device>(&device) };
    unsafe {
        out.write(null_mut());
        (bound.vtable().CreateFence)(device, 0, D3D12_FENCE_FLAG_NONE, &ID3D12Fence::IID, out)
    }
    .0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_create_event(initial: u32) -> Raw {
    unsafe { CreateEventW(null(), 0, i32::from(initial != 0), null()) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_duplicate_event(event: Raw) -> Raw {
    let process = unsafe { GetCurrentProcess() };
    let mut copy = null_mut();
    unsafe {
        DuplicateHandle(
            process,
            event,
            process,
            &mut copy,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    copy
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_close_event(event: Raw) {
    unsafe { CloseHandle(event) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_create_slot(
    device: Raw,
    out_allocator: *mut Raw,
    out_list: *mut Raw,
) -> i32 {
    unsafe {
        out_allocator.write(null_mut());
        out_list.write(null_mut());
    }
    let bound = unsafe { borrow::<ID3D12Device>(&device) };
    let mut allocator = null_mut();
    let mut list = null_mut();
    let ty = D3D12_COMMAND_LIST_TYPE_DIRECT;
    let allocated = unsafe {
        (bound.vtable().CreateCommandAllocator)(
            device,
            ty,
            &ID3D12CommandAllocator::IID,
            &mut allocator,
        )
    };
    let owner = unsafe { ComRef::from_owned(allocator) };
    if !allocated.is_ok() || owner.is_none() {
        return E_FAIL;
    }
    let created = unsafe {
        (bound.vtable().CreateCommandList)(
            device,
            0,
            ty,
            allocator,
            null_mut(),
            &ID3D12GraphicsCommandList::IID,
            &mut list,
        )
    };
    let recording = unsafe { ComRef::from_owned(list) };
    let (Some(owner), Some(recording)) = (owner, recording) else {
        return E_FAIL;
    };
    if !created.is_ok() || unsafe { borrow::<ID3D12GraphicsCommandList>(&list).Close() }.is_err() {
        return E_FAIL;
    }
    unsafe {
        out_allocator.write(owner.into_raw());
        out_list.write(recording.into_raw());
    }
    S_OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_render_buffer(
    device: Raw,
    width: u32,
    height: u32,
    format: u32,
    out: *mut Raw,
) -> i32 {
    let bound = unsafe { borrow::<ID3D12Device>(&device) };
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
        Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        ..Default::default()
    };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    unsafe {
        out.write(null_mut());
        (bound.vtable().CreateCommittedResource)(
            device,
            &heap,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_PRESENT,
            null(),
            &ID3D12Resource::IID,
            out,
        )
    }
    .0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_signal(queue: Raw, fence: Raw, value: u64) -> i32 {
    let bound = unsafe { borrow::<ID3D12CommandQueue>(&queue) };
    unsafe { (bound.vtable().Signal)(queue, fence, value) }.0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_join(source: Raw, target: Raw, fence: Raw, value: u64) -> i32 {
    let signaled = unsafe { rsf_d3d11p_gpu_signal(source, fence, value) };
    if signaled < 0 {
        return signaled;
    }
    let bound = unsafe { borrow::<ID3D12CommandQueue>(&target) };
    unsafe { (bound.vtable().Wait)(target, fence, value) }.0
}

unsafe extern "C" fn completed(fence: Raw) -> u64 {
    unsafe { borrow::<ID3D12Fence>(&fence).GetCompletedValue() }
}

unsafe extern "C" fn arm(fence: Raw, value: u64, event: Raw) -> i32 {
    let bound = unsafe { borrow::<ID3D12Fence>(&fence) };
    unsafe { (bound.vtable().SetEventOnCompletion)(fence, value, HANDLE(event)) }.0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_wait(fence: Raw, event: Raw, value: u64) -> i32 {
    unsafe {
        crate::d3d12_fence::rsf_core_d3d12_wait_fence(fence, value, event, 10_000, completed, arm)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_signal_wait(
    queue: Raw,
    fence: Raw,
    event: Raw,
    value: u64,
) -> i32 {
    if unsafe { rsf_d3d11p_gpu_signal(queue, fence, value) } < 0 {
        return E_FAIL;
    }
    unsafe { rsf_d3d11p_gpu_wait(fence, event, value) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_reset_slot(
    fence: Raw,
    event: Raw,
    allocator: Raw,
    list: Raw,
    value: u64,
    step: *mut u32,
) -> i32 {
    unsafe { step.write(0) };
    let waited = unsafe { rsf_d3d11p_gpu_wait(fence, event, value) };
    if waited < 0 {
        return waited;
    }
    unsafe { step.write(1) };
    let bound = unsafe { borrow::<ID3D12CommandAllocator>(&allocator) };
    let reset = unsafe { (bound.vtable().Reset)(allocator) };
    if !reset.is_ok() {
        return reset.0;
    }
    unsafe { step.write(2) };
    let bound = unsafe { borrow::<ID3D12GraphicsCommandList>(&list) };
    unsafe { (bound.vtable().Reset)(list, allocator, null_mut()) }.0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_close_execute(queue: Raw, list: Raw, command: Raw) -> i32 {
    let bound = unsafe { borrow::<ID3D12GraphicsCommandList>(&list) };
    let closed = unsafe { (bound.vtable().Close)(list) };
    if closed.is_ok() {
        let bound = unsafe { borrow::<ID3D12CommandQueue>(&queue) };
        unsafe { (bound.vtable().ExecuteCommandLists)(queue, 1, &command) };
    }
    closed.0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_application_ready(event: Raw, fence: Raw, value: u64) -> i32 {
    if !fence.is_null() {
        return unsafe { arm(fence, value, event) };
    }
    if unsafe { SetEvent(event) } != 0 {
        S_OK
    } else {
        E_FAIL
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_buffer_referenced(resource: Raw) -> u32 {
    let bound = unsafe { borrow::<IUnknown>(&resource) };
    unsafe {
        (bound.vtable().AddRef)(resource);
        u32::from((bound.vtable().Release)(resource) > 1)
    }
}

unsafe fn transition(
    list: &ID3D12GraphicsCommandList,
    resource: Raw,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
    if before == after {
        return;
    }
    // The active facade lease owns the texture. The descriptor borrows it.
    let barrier = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: ManuallyDrop::new(Some(unsafe { ID3D12Resource::from_raw(resource) })),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    };
    unsafe { list.ResourceBarrier(&[barrier]) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_gpu_record_copy(
    list: Raw,
    destination: Raw,
    source: Raw,
    native12: u32,
) {
    let bound = unsafe { borrow::<ID3D12GraphicsCommandList>(&list) };
    let source_state = if native12 != 0 {
        D3D12_RESOURCE_STATE_PRESENT
    } else {
        D3D12_RESOURCE_STATE_COMMON
    };
    unsafe {
        transition(
            bound,
            source,
            source_state,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        );
        transition(
            bound,
            destination,
            D3D12_RESOURCE_STATE_PRESENT,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        (bound.vtable().CopyResource)(list, destination, source);
        transition(
            bound,
            destination,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATE_PRESENT,
        );
        transition(
            bound,
            source,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            source_state,
        );
    }
}
