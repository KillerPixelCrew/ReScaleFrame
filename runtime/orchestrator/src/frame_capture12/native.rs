// SPDX-License-Identifier: MIT
use super::{Handle, Layout};
use crate::d3d11_d3d12::transition_barrier;
use std::mem::ManuallyDrop;
use std::ptr;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::core::Interface;
// The fence leaves only the capture owner's production `ops` reach.
#[cfg(not(test))]
use {
    super::SUBMITTED,
    crate::d3d12_fence::wait_fence,
    windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_REMOVED,
    windows_sys::Win32::{Foundation::CloseHandle, System::Threading::CreateEventW},
};

mod xess;

#[cfg_attr(test, allow(dead_code))]
fn allocate(source: &ID3D12Resource, single_slice: bool) -> Option<(ID3D12Resource, Layout)> {
    let desc = unsafe { source.GetDesc() };
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.SampleDesc.Count != 1
        || (single_slice && desc.DepthOrArraySize != 1)
    {
        return None;
    }
    let mut device: Option<ID3D12Device> = None;
    unsafe { source.GetDevice(&mut device) }.ok()?;
    let device = device?;
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
        );
    }
    let buffer = readback_buffer(&device, bytes)?;
    let fields = footprint.Footprint;
    Some((
        buffer,
        Layout {
            bytes,
            offset: footprint.Offset,
            width: fields.Width,
            height: fields.Height,
            format: fields.Format.0 as u32,
            row_pitch: fields.RowPitch,
        },
    ))
}

fn readback_buffer(device: &ID3D12Device, bytes: u64) -> Option<ID3D12Resource> {
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_READBACK,
        ..Default::default()
    };
    let storage = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: bytes,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let mut buffer = None;
    unsafe {
        device.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_NONE,
            &storage,
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
            &mut buffer,
        )
    }
    .ok()?;
    buffer
}

// A readback buffer, with one reference, and where the copy's rows land in it.
#[cfg_attr(test, allow(dead_code))]
unsafe fn allocate_raw(source: Handle, single_slice: bool) -> Option<(Handle, Layout)> {
    let source = unsafe { ID3D12Resource::from_raw_borrowed(&source) }?;
    let (buffer, layout) = allocate(source, single_slice)?;
    Some((buffer.into_raw(), layout))
}

#[cfg(not(test))]
pub(super) unsafe fn allocate_capture(source: Handle) -> Option<(Handle, Layout)> {
    unsafe { allocate_raw(source, true) }
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) unsafe fn rsf_fg12_platform_readback(source: Handle) -> Option<(Handle, Layout)> {
    unsafe { allocate_raw(source, false) }
}

fn resource_field(resource: &ID3D12Resource) -> ManuallyDrop<Option<ID3D12Resource>> {
    // The descriptor borrows the live reference held by the recording owner.
    ManuallyDrop::new(Some(unsafe { ID3D12Resource::from_raw(resource.as_raw()) }))
}

fn transition(
    commands: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    from: D3D12_RESOURCE_STATES,
    to: D3D12_RESOURCE_STATES,
) {
    if from == to {
        return;
    }
    let barrier = transition_barrier(resource, from, to);
    unsafe { commands.ResourceBarrier(&[barrier]) };
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) unsafe extern "C" fn rsf_fg12_platform_readback_copy(
    list: Handle,
    source: Handle,
    state: u32,
    buffer: Handle,
    layout: *const Layout,
) {
    let (Some(commands), Some(source), Some(buffer), Some(layout)) = (unsafe {
        (
            ID3D12GraphicsCommandList::from_raw_borrowed(&list),
            ID3D12Resource::from_raw_borrowed(&source),
            ID3D12Resource::from_raw_borrowed(&buffer),
            layout.as_ref(),
        )
    }) else {
        return;
    };
    let before = D3D12_RESOURCE_STATES(state as i32);
    let copy = D3D12_RESOURCE_STATE_COPY_SOURCE;
    let from = D3D12_TEXTURE_COPY_LOCATION {
        pResource: resource_field(source),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: 0,
        },
    };
    let to = D3D12_TEXTURE_COPY_LOCATION {
        pResource: resource_field(buffer),
        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                Offset: layout.offset,
                Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                    Format: DXGI_FORMAT(layout.format as i32),
                    Width: layout.width,
                    Height: layout.height,
                    Depth: 1,
                    RowPitch: layout.row_pitch,
                },
            },
        },
    };
    transition(commands, source, before, copy);
    unsafe { commands.CopyTextureRegion(&to, 0, 0, 0, &from, None) };
    transition(commands, source, copy, before);
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) unsafe extern "C" fn rsf_fg12_platform_map(buffer: Handle, bytes: u64) -> *const u8 {
    let Some(buffer) = (unsafe { ID3D12Resource::from_raw_borrowed(&buffer) }) else {
        return ptr::null();
    };
    let mut data = ptr::null_mut();
    let range = D3D12_RANGE {
        Begin: 0,
        End: bytes as usize,
    };
    if unsafe { buffer.Map(0, Some(&range), Some(&mut data)) }.is_ok() {
        data.cast()
    } else {
        ptr::null()
    }
}

pub(crate) unsafe extern "C" fn rsf_fg12_platform_unmap(buffer: Handle) {
    if let Some(buffer) = unsafe { ID3D12Resource::from_raw_borrowed(&buffer) } {
        let written = D3D12_RANGE { Begin: 0, End: 0 };
        unsafe { buffer.Unmap(0, Some(&written)) };
    }
}

#[cfg(not(test))]
fn new_fence(queue: &ID3D12CommandQueue) -> Option<ID3D12Fence> {
    let mut device: Option<ID3D12Device> = None;
    unsafe { queue.GetDevice(&mut device) }.ok()?;
    unsafe { device?.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.ok()
}

#[cfg(not(test))]
pub(super) unsafe extern "C" fn signal(queue: Handle) -> Handle {
    let fence = (unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }).and_then(|queue| {
        let fence = new_fence(queue)?;
        unsafe { queue.Signal(&fence, SUBMITTED) }.ok()?;
        Some(fence)
    });
    fence.map_or(ptr::null_mut(), Interface::into_raw)
}

#[cfg(not(test))]
pub(super) unsafe extern "C" fn completed(fence: Handle) -> u64 {
    (unsafe { ID3D12Fence::from_raw_borrowed(&fence) })
        .map_or(0, |fence| unsafe { fence.GetCompletedValue() })
}

#[cfg(not(test))]
pub(super) unsafe extern "C" fn drain(queue: Handle) -> u64 {
    let Some(queue) = (unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }) else {
        return 0;
    };
    let Some(fence) = new_fence(queue) else {
        return 0;
    };
    let event = unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) };
    if event.is_null() {
        return 0;
    }
    let result = unsafe { queue.Signal(&fence, SUBMITTED) }.map_or_else(
        |error| error.code().0,
        |()| unsafe { wait_fence(fence.as_raw(), SUBMITTED, event, 15_000) },
    );
    unsafe { CloseHandle(event) };
    if result == DXGI_ERROR_DEVICE_REMOVED.0 {
        u64::MAX
    } else if result >= 0 {
        SUBMITTED
    } else {
        0
    }
}

// The leaves frame_capture12.rs calls as `ops::*`; its tests fake the same names.
// `allocate` refuses all but a single-sample, single-slice 2D texture and returns a readback
// buffer in COPY_DEST with one reference; `signal` returns a new fence signalled to 1 on the
// queue; `drain` signals the queue, waits at most 15 seconds and returns the fence value on a
// completed wait, zero on a refused one.
#[cfg(not(test))]
pub(super) mod ops {
    pub(in crate::frame_capture12) use super::{
        allocate_capture as allocate, completed, drain, rsf_fg12_platform_map as map,
        rsf_fg12_platform_readback_copy as copy, rsf_fg12_platform_unmap as unmap, signal,
    };
    pub(in crate::frame_capture12) use crate::input_dump::abi::{
        rsf_input_dump_failed as failed, rsf_input_dump_pixels as pixels,
        rsf_input_dump_reserve as reserve, rsf_input_dump_wants as wants,
    };
}
