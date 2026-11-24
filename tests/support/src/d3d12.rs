// SPDX-License-Identifier: MIT
//! D3D12 resource, barrier and copy-location helpers the fixture crates share.
use std::ffi::CStr;
use std::mem::ManuallyDrop;
use windows::Win32::Foundation::{CloseHandle, E_POINTER, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::{IUnknown, Interface, PCWSTR, Param, Result};

/// A feature level 11.0 device on `adapter` and a direct command queue on it.
pub fn device_and_queue(
    adapter: impl Param<IUnknown>,
) -> Result<(ID3D12Device, ID3D12CommandQueue)> {
    let device = crate::d3d12_device(adapter)?;
    let queue = unsafe {
        device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            ..Default::default()
        })
    }?;
    Ok((device, queue))
}

/// A non-owning reference for the `ManuallyDrop<Option<ID3D12Resource>>` fields of barrier and
/// copy-location records. The caller keeps `resource` alive until the GPU work that names it ends.
pub fn borrowed(resource: &ID3D12Resource) -> ManuallyDrop<Option<ID3D12Resource>> {
    ManuallyDrop::new(Some(unsafe { ID3D12Resource::from_raw(resource.as_raw()) }))
}

/// A committed resource of `desc` in a `heap` of `device`, created in `state`.
pub fn committed(
    device: &ID3D12Device,
    heap: D3D12_HEAP_TYPE,
    desc: &D3D12_RESOURCE_DESC,
    state: D3D12_RESOURCE_STATES,
) -> Result<ID3D12Resource> {
    let properties = D3D12_HEAP_PROPERTIES {
        Type: heap,
        ..Default::default()
    };
    let mut resource = None;
    unsafe {
        device.CreateCommittedResource(
            &properties,
            D3D12_HEAP_FLAG_NONE,
            desc,
            state,
            None,
            &mut resource,
        )
    }?;
    resource.ok_or_else(|| E_POINTER.into())
}

/// A `bytes` buffer in `heap`: `COPY_DEST` in the readback heap, `GENERIC_READ` otherwise.
pub fn buffer(device: &ID3D12Device, bytes: u64, heap: D3D12_HEAP_TYPE) -> Result<ID3D12Resource> {
    let desc = D3D12_RESOURCE_DESC {
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
    let state = if heap == D3D12_HEAP_TYPE_READBACK {
        D3D12_RESOURCE_STATE_COPY_DEST
    } else {
        D3D12_RESOURCE_STATE_GENERIC_READ
    };
    committed(device, heap, &desc, state)
}

/// The placed footprint and total bytes of subresource 0 of `desc`.
pub fn footprint(
    device: &ID3D12Device,
    desc: &D3D12_RESOURCE_DESC,
) -> (D3D12_PLACED_SUBRESOURCE_FOOTPRINT, u64) {
    let (mut footprint, mut bytes) = (D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default(), 0);
    unsafe {
        device.GetCopyableFootprints(
            desc,
            0,
            1,
            0,
            Some(&mut footprint),
            None,
            None,
            Some(&mut bytes),
        )
    };
    (footprint, bytes)
}

/// A copy location of `resource`: the placed `footprint` of a buffer, or subresource 0 of a
/// texture. See [`borrowed`] for the lifetime.
pub fn copy_location(
    resource: &ID3D12Resource,
    footprint: Option<D3D12_PLACED_SUBRESOURCE_FOOTPRINT>,
) -> D3D12_TEXTURE_COPY_LOCATION {
    let (kind, details) = match footprint {
        Some(footprint) => (
            D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: footprint,
            },
        ),
        None => (
            D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: 0,
            },
        ),
    };
    D3D12_TEXTURE_COPY_LOCATION {
        pResource: borrowed(resource),
        Type: kind,
        Anonymous: details,
    }
}

/// A transition barrier of every subresource of `resource`. See [`borrowed`] for the lifetime.
pub fn transition_barrier(
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: borrowed(resource),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

/// Signals `fence` to `value` on `queue` and waits up to `timeout_ms` for the GPU to reach it.
pub fn wait_for_signal(
    queue: &ID3D12CommandQueue,
    fence: &ID3D12Fence,
    value: u64,
    timeout_ms: u32,
) -> bool {
    let Ok(event) = (unsafe { CreateEventW(None, false, false, PCWSTR::null()) }) else {
        return false;
    };
    let reached = unsafe {
        queue.Signal(fence, value).is_ok()
            && fence.SetEventOnCompletion(value, event).is_ok()
            && WaitForSingleObject(event, timeout_ms) == WAIT_OBJECT_0
    };
    let _ = unsafe { CloseHandle(event) };
    reached
}

/// The messages stored in `device`'s info queue, oldest first, as (severity, description). `None`
/// when the device has no info queue because the debug layer is off.
pub fn debug_messages(device: &ID3D12Device) -> Option<Vec<(D3D12_MESSAGE_SEVERITY, String)>> {
    let queue = device.cast::<ID3D12InfoQueue>().ok()?;
    let mut messages = Vec::new();
    for index in 0..unsafe { queue.GetNumStoredMessagesAllowedByRetrievalFilter() } {
        let mut bytes = 0;
        if unsafe { queue.GetMessage(index, None, &mut bytes) }.is_err()
            || bytes < size_of::<D3D12_MESSAGE>()
        {
            continue;
        }
        // Word-aligned storage holds the record and the description string appended to it.
        let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        let message = storage.as_mut_ptr().cast::<D3D12_MESSAGE>();
        if unsafe { queue.GetMessage(index, Some(message), &mut bytes) }.is_ok() {
            let message = unsafe { &*message };
            let text = (!message.pDescription.is_null())
                .then(|| unsafe { CStr::from_ptr(message.pDescription.cast()) })
                .map(|text| text.to_string_lossy().into_owned());
            messages.push((message.Severity, text.unwrap_or_default()));
        }
    }
    Some(messages)
}

/// The descriptions of the error and corruption messages in `device`'s info queue. `None` when the
/// device has no info queue.
pub fn validation_errors(device: &ID3D12Device) -> Option<Vec<String>> {
    let messages = debug_messages(device)?;
    Some(
        messages
            .into_iter()
            .filter(|(severity, _)| severity.0 <= D3D12_MESSAGE_SEVERITY_ERROR.0)
            .map(|(_, text)| text)
            .collect(),
    )
}

/// Records [`transition_barrier`] on `list`.
pub fn transition(
    list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
    unsafe { list.ResourceBarrier(&[transition_barrier(resource, before, after)]) };
}
