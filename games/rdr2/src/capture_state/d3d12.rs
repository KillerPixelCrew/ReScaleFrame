// SPDX-License-Identifier: MIT
//! Win32 graphics leaves for leased capture copies and completion.
use super::artifacts::write_image;
use super::gpu_leases::{
    Ops, ReadbackLeases, gpu_adopt, gpu_arm_backbuffers, gpu_bind_fence, gpu_selected,
};
use super::metadata::PresentationDesc;
use super::shots::Image;
use super::{Capture, active, arm, can_arm, end_draws};
use std::ffi::{c_char, c_void};
use std::mem::ManuallyDrop;
use std::ptr::null_mut;
use windows::Win32::Graphics::{
    Direct3D12::*,
    Dxgi::{IDXGISwapChain, IDXGISwapChain3},
};
use windows::core::{IUnknown, Interface};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

static GPU_OPS: Ops = Ops {
    retain,
    release,
    create_fence,
    signal,
    get_buffer,
};

pub(super) fn d3d12_ops() -> &'static Ops {
    &GPU_OPS
}

unsafe extern "C" fn retain(object: *mut c_void) {
    let object = unsafe { IUnknown::from_raw_borrowed(&object) }.expect("live capture object");
    std::mem::forget(IUnknown::clone(object));
}

unsafe extern "C" fn release(object: *mut c_void) {
    drop(unsafe { IUnknown::from_raw(object) });
}

unsafe fn device(child: &ID3D12DeviceChild) -> Option<ID3D12Device> {
    let mut device = None;
    unsafe { child.GetDevice(&mut device) }.ok()?;
    device
}

unsafe extern "C" fn create_fence(queue: *mut c_void) -> *mut c_void {
    let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) };
    queue
        .and_then(|queue| unsafe { device(queue) })
        .and_then(|device| {
            unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) }.ok()
        })
        .map_or_else(null_mut, Interface::into_raw)
}

unsafe extern "C" fn signal(queue: *mut c_void, fence: *mut c_void, value: u64) -> bool {
    let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) };
    let fence = unsafe { ID3D12Fence::from_raw_borrowed(&fence) };
    queue
        .zip(fence)
        .is_some_and(|(queue, fence)| unsafe { queue.Signal(fence, value) }.is_ok())
}

unsafe extern "C" fn get_buffer(chain: *mut c_void, index: u32) -> *mut c_void {
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetBuffer::<ID3D12Resource>(index) }.ok())
        .map_or_else(null_mut, Interface::into_raw)
}

pub(super) unsafe fn d3d12_completed(fence: *mut c_void) -> u64 {
    unsafe { ID3D12Fence::from_raw_borrowed(&fence) }
        .map_or(0, |fence| unsafe { fence.GetCompletedValue() })
}

pub(super) unsafe fn d3d12_removed(buffer: *mut c_void) -> bool {
    unsafe { ID3D12Resource::from_raw_borrowed(&buffer) }
        .and_then(|buffer| unsafe { device(buffer) })
        .is_some_and(|device| unsafe { device.GetDeviceRemovedReason() }.is_err())
}

/// The command-list caller keeps both resources live through recording. D3D12's
/// copy and barrier structs borrow the pointers without taking ownership.
unsafe fn borrowed_resource(pointer: *mut c_void) -> ManuallyDrop<Option<ID3D12Resource>> {
    ManuallyDrop::new(Some(unsafe { ID3D12Resource::from_raw(pointer) }))
}

unsafe fn barrier(
    list: &ID3D12GraphicsCommandList,
    source: *mut c_void,
    from: D3D12_RESOURCE_STATES,
    to: D3D12_RESOURCE_STATES,
) {
    if from != to {
        let transition = D3D12_RESOURCE_TRANSITION_BARRIER {
            pResource: unsafe { borrowed_resource(source) },
            Subresource: 0,
            StateBefore: from,
            StateAfter: to,
        };
        unsafe {
            list.ResourceBarrier(&[D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    Transition: ManuallyDrop::new(transition),
                },
            }]);
        }
    }
}

/// The native caller suppresses its reentrant barrier and allocation observers
/// while recording a capture copy. The Rust shot owns the returned buffer.
pub(super) unsafe fn d3d12_copy(
    list: *mut c_void,
    source: *mut c_void,
    state: u32,
    leases: &mut ReadbackLeases,
    image: &mut Image,
    allocation_hresult: &mut i32,
) -> bool {
    let Some(list) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }) else {
        return false;
    };
    let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&source) }) else {
        return false;
    };
    let desc = unsafe { resource.GetDesc() };
    let Ok(width) = u32::try_from(desc.Width) else {
        return false;
    };
    if state == u32::MAX
        || desc.SampleDesc.Count != 1
        || desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.DepthOrArraySize != 1
    {
        return false;
    }
    let Some(device) = (unsafe { device(resource) }) else {
        return false;
    };
    let mut footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
    let mut bytes = 0;
    unsafe {
        device.GetCopyableFootprints(
            &desc,
            0,
            1,
            0,
            Some(&mut footprint),
            None,
            None,
            Some(&mut bytes),
        )
    };
    // The raw image includes the last row's padding.
    if bytes == u64::MAX || footprint.Offset != 0 {
        return false;
    }
    let padded_rows =
        u64::from(footprint.Footprint.RowPitch) * u64::from(footprint.Footprint.Height);
    let Some(padded_bytes) = footprint.Offset.checked_add(padded_rows) else {
        return false;
    };
    let bytes = bytes.max(padded_bytes);
    if let Some(buffer) = leases.buffer {
        // The retained tail can be reused only when its buffer can hold this copy.
        let raw = buffer.as_ptr();
        let buffer = unsafe { ID3D12Resource::from_raw_borrowed(&raw) }
            .map(|buffer| unsafe { buffer.GetDesc() });
        if buffer.is_none_or(|desc| desc.Width < bytes) {
            return false;
        }
    } else {
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_READBACK,
            ..Default::default()
        };
        let destination = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: bytes,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let mut buffer: Option<ID3D12Resource> = None;
        if let Err(error) = unsafe {
            device.CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &destination,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut buffer,
            )
        } {
            *allocation_hresult = error.code().0;
            return false;
        }
        let Some(buffer) = buffer else {
            return false;
        };
        if !gpu_adopt(leases, buffer.into_raw(), &GPU_OPS) {
            return false;
        }
    }
    let Some(buffer) = leases.buffer else {
        return false;
    };
    gpu_bind_fence(leases, null_mut());
    let before = D3D12_RESOURCE_STATES(state as i32);
    unsafe {
        barrier(list, source, before, D3D12_RESOURCE_STATE_COPY_SOURCE);
        let src = D3D12_TEXTURE_COPY_LOCATION {
            pResource: borrowed_resource(source),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: 0,
            },
        };
        let dst = D3D12_TEXTURE_COPY_LOCATION {
            pResource: borrowed_resource(buffer.as_ptr()),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: footprint,
            },
        };
        list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
        barrier(list, source, D3D12_RESOURCE_STATE_COPY_SOURCE, before);
    }
    image.width = width;
    image.height = desc.Height;
    image.format = desc.Format.0 as u32;
    image.pitch = footprint.Footprint.RowPitch;
    image.footprint_format = footprint.Footprint.Format.0 as u32;
    image.state_before = state;
    image.resource = source.addr() as u64;
    true
}

/// The worker calls this only after completion has admitted the shot for mapping.
pub(super) unsafe fn d3d12_write(
    leases: &ReadbackLeases,
    image: &Image,
    directory: *const u16,
    directory_len: usize,
    pid: u32,
    role: *const c_char,
    map_hresult: &mut i32,
) -> bool {
    let Some(buffer) = leases.buffer else {
        return false;
    };
    let raw = buffer.as_ptr();
    let Some(buffer) = (unsafe { ID3D12Resource::from_raw_borrowed(&raw) }) else {
        return false;
    };
    let Some(bytes) = (image.pitch as usize).checked_mul(image.height as usize) else {
        return false;
    };
    let range = D3D12_RANGE {
        Begin: 0,
        End: bytes,
    };
    let mut data = null_mut();
    if let Err(error) = unsafe { buffer.Map(0, Some(&range), Some(&mut data)) } {
        *map_hresult = error.code().0;
        return false;
    }
    let saved = unsafe {
        write_image(
            image,
            directory,
            directory_len,
            pid,
            role,
            data.cast(),
            bytes,
        )
    };
    unsafe { buffer.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 })) };
    saved
}

/// The native observer has verified the presenting owner and holds the capture mutex.
pub(super) unsafe fn d3d12_present(
    owner: &Capture,
    chain: *mut c_void,
    completed_target: &mut *mut c_void,
) -> bool {
    let Some(swapchain) = (unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }) else {
        return false;
    };
    let selected = unsafe { gpu_selected(owner, chain) };
    if active(owner)
        && !selected.is_null()
        && swapchain.cast::<IDXGISwapChain3>().is_ok_and(|swapchain| {
            unsafe { swapchain.GetBuffer::<ID3D12Resource>(swapchain.GetCurrentBackBufferIndex()) }
                .is_ok_and(|current| current.as_raw() == selected)
        })
    {
        end_draws(owner);
        *completed_target = selected;
    }
    if !unsafe { can_arm(owner) } {
        return false;
    }
    let Ok(desc) = (unsafe { swapchain.GetDesc() }) else {
        return false;
    };
    let identity = swapchain.cast::<IUnknown>().ok();
    let identity = identity.as_ref().map_or(chain, Interface::as_raw);
    if !unsafe { gpu_arm_backbuffers(owner, chain, desc.BufferCount, &GPU_OPS) } {
        return false;
    }
    let descriptor = PresentationDesc {
        format: desc.BufferDesc.Format.0 as u32,
        width: desc.BufferDesc.Width,
        height: desc.BufferDesc.Height,
    };
    unsafe { arm(owner, GetTickCount64(), identity, &descriptor) };
    true
}
