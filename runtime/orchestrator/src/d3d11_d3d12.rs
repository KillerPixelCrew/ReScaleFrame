// SPDX-License-Identifier: MIT
//! Windows operations shared by the D3D11-to-D3D12 SR and FG paths.
use crate::com_ref;
use crate::shared_surface::{
    rsf_shared_fence_d3d11, rsf_shared_fence_d3d12, rsf_shared_surface_d3d11,
    rsf_shared_surface_d3d12, rsf_shared_surface_destroy,
};
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::ptr::{null, null_mut};
use windows::Win32::{
    Graphics::{Direct3D11::*, Direct3D12::*, Dxgi::Common::*},
};
use windows::core::IUnknown;
use windows::core::{HRESULT, Interface};
use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::CreateEventW};

type Handle = *mut c_void;

pub(crate) unsafe fn borrow<T: Interface>(raw: &Handle) -> &T {
    // The graphics owner supplies a live interface borrowed from an active lease.
    unsafe { T::from_raw_borrowed(raw) }.expect("live graphics interface")
}

pub(crate) fn output(create: impl FnOnce(*mut Handle) -> HRESULT) -> Option<IUnknown> {
    let mut raw = null_mut();
    let result = create(&mut raw);
    unsafe { com_ref::from_result(result, raw) }
}

pub(crate) unsafe extern "C" fn query<S: Interface, T: Interface>(object: Handle) -> Handle {
    let source = unsafe { borrow::<S>(&object) };
    output(|out| unsafe { source.query(&T::IID, out) }).map_or(null_mut(), Interface::into_raw)
}

pub(crate) unsafe extern "C" fn create_event() -> Handle {
    unsafe { CreateEventW(null(), 0, 0, null()) }
}

pub(crate) unsafe extern "C" fn close_event(event: Handle) {
    unsafe { CloseHandle(event) };
}

pub(crate) unsafe extern "C" fn surface_destroy(surface: Handle) {
    unsafe { rsf_shared_surface_destroy(surface.cast()) };
}

pub(crate) unsafe extern "C" fn surface_d3d11(surface: Handle) -> Handle {
    unsafe { rsf_shared_surface_d3d11(surface.cast()) }
}

pub(crate) unsafe extern "C" fn surface_d3d12(surface: Handle) -> Handle {
    unsafe { rsf_shared_surface_d3d12(surface.cast()) }
}

pub(crate) unsafe extern "C" fn fence_d3d11(fence: Handle) -> Handle {
    unsafe { rsf_shared_fence_d3d11(fence.cast()) }
}

pub(crate) unsafe extern "C" fn fence_d3d12(fence: Handle) -> Handle {
    unsafe { rsf_shared_fence_d3d12(fence.cast()) }
}

pub(crate) unsafe extern "C" fn flush(context: Handle) {
    unsafe { borrow::<ID3D11DeviceContext4>(&context).Flush() };
}

pub(crate) unsafe extern "C" fn copy_resource(
    context: Handle,
    destination: Handle,
    source: Handle,
) {
    unsafe {
        borrow::<ID3D11DeviceContext4>(&context).CopyResource(
            borrow::<ID3D11Resource>(&destination),
            borrow::<ID3D11Resource>(&source),
        );
    }
}

pub(crate) unsafe extern "C" fn context_signal(context: Handle, fence: Handle, value: u64) -> i32 {
    i32::from(
        unsafe {
            borrow::<ID3D11DeviceContext4>(&context).Signal(borrow::<ID3D11Fence>(&fence), value)
        }
        .is_ok(),
    )
}

pub(crate) unsafe extern "C" fn context_wait(context: Handle, fence: Handle, value: u64) -> i32 {
    i32::from(
        unsafe {
            borrow::<ID3D11DeviceContext4>(&context).Wait(borrow::<ID3D11Fence>(&fence), value)
        }
        .is_ok(),
    )
}

pub(crate) unsafe extern "C" fn create_commands(
    device: Handle,
    allocator: *mut Handle,
    list: *mut Handle,
) -> i32 {
    let native = unsafe { borrow::<ID3D12Device>(&device) };
    let kind = D3D12_COMMAND_LIST_TYPE_DIRECT;
    let Some(new_allocator) = output(|out| unsafe {
        (native.vtable().CreateCommandAllocator)(device, kind, &ID3D12CommandAllocator::IID, out)
    }) else {
        return 0;
    };
    let Some(new_list) = output(|out| unsafe {
        (native.vtable().CreateCommandList)(
            device,
            0,
            kind,
            new_allocator.as_raw(),
            null_mut(),
            &ID3D12GraphicsCommandList::IID,
            out,
        )
    }) else {
        return 0;
    };
    if unsafe { close_commands(new_list.as_raw()) } == 0 {
        return 0;
    }
    unsafe {
        allocator.write(new_allocator.into_raw());
        list.write(new_list.into_raw());
    }
    1
}

pub(crate) unsafe extern "C" fn create_output(
    device: Handle,
    width: u32,
    height: u32,
    format: u32,
) -> Handle {
    unsafe {
        create_texture12(
            device,
            width,
            height,
            format,
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        )
    }
}

pub(crate) unsafe fn create_texture12(
    device: Handle,
    width: u32,
    height: u32,
    format: u32,
    flags: D3D12_RESOURCE_FLAGS,
    state: D3D12_RESOURCE_STATES,
) -> Handle {
    let native = unsafe { borrow::<ID3D12Device>(&device) };
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
        Format: DXGI_FORMAT(format as i32),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Flags: flags,
        ..Default::default()
    };
    output(|out| unsafe {
        (native.vtable().CreateCommittedResource)(
            device,
            &heap,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            state,
            null(),
            &ID3D12Resource::IID,
            out,
        )
    })
    .map_or(null_mut(), Interface::into_raw)
}

pub(crate) unsafe extern "C" fn reset_commands(allocator: Handle, list: Handle) -> i32 {
    let allocator = unsafe { borrow::<ID3D12CommandAllocator>(&allocator) };
    i32::from(
        unsafe {
            allocator.Reset().and_then(|()| {
                borrow::<ID3D12GraphicsCommandList>(&list)
                    .Reset(allocator, None::<&ID3D12PipelineState>)
            })
        }
        .is_ok(),
    )
}

pub(crate) unsafe extern "C" fn close_commands(list: Handle) -> i32 {
    i32::from(unsafe { borrow::<ID3D12GraphicsCommandList>(&list).Close() }.is_ok())
}

pub(crate) unsafe extern "C" fn execute(queue: Handle, list: Handle) {
    let queue = unsafe { borrow::<ID3D12CommandQueue>(&queue) };
    // The list stays borrowed for ExecuteCommandLists. No reference is transferred.
    let lists = ManuallyDrop::new([Some(unsafe { ID3D12CommandList::from_raw(list) })]);
    unsafe { queue.ExecuteCommandLists(&lists[..]) };
}

pub(crate) unsafe extern "C" fn transition(
    list: Handle,
    resource: Handle,
    before: u32,
    after: u32,
) {
    if before == after {
        return;
    }
    let barrier = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: ManuallyDrop::new(Some(unsafe { ID3D12Resource::from_raw(resource) })),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: D3D12_RESOURCE_STATES(before as i32),
                StateAfter: D3D12_RESOURCE_STATES(after as i32),
            }),
        },
    };
    unsafe { borrow::<ID3D12GraphicsCommandList>(&list).ResourceBarrier(&[barrier]) };
}

pub(crate) unsafe extern "C" fn copy_transitioned(
    list: Handle,
    destination: Handle,
    before_dst: u32,
    after_dst: u32,
    source: Handle,
    before_src: u32,
    after_src: u32,
) {
    let copy_src = D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32;
    let copy_dst = D3D12_RESOURCE_STATE_COPY_DEST.0 as u32;
    unsafe {
        transition(list, source, before_src, copy_src);
        transition(list, destination, before_dst, copy_dst);
        borrow::<ID3D12GraphicsCommandList>(&list).CopyResource(
            borrow::<ID3D12Resource>(&destination),
            borrow::<ID3D12Resource>(&source),
        );
        transition(list, destination, copy_dst, after_dst);
        transition(list, source, copy_src, after_src);
    }
}

pub(crate) unsafe extern "C" fn queue_signal(queue: Handle, fence: Handle, value: u64) -> i32 {
    i32::from(
        unsafe {
            borrow::<ID3D12CommandQueue>(&queue).Signal(borrow::<ID3D12Fence>(&fence), value)
        }
        .is_ok(),
    )
}

pub(crate) unsafe extern "C" fn queue_wait(queue: Handle, fence: Handle, value: u64) -> i32 {
    i32::from(
        unsafe { borrow::<ID3D12CommandQueue>(&queue).Wait(borrow::<ID3D12Fence>(&fence), value) }
            .is_ok(),
    )
}
