// SPDX-License-Identifier: MIT
use rsf_game_sys::{depth_domains_requested, rsf_game_render_pass};
use std::ffi::c_void;
use std::ptr::null_mut;
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::*};
use windows::core::{GUID, HRESULT, Interface};

pub(super) type Raw = *mut c_void;

#[cfg_attr(test, allow(dead_code))]
pub(super) unsafe extern "C" fn pass_depth_domains(pass: *const rsf_game_render_pass) -> u32 {
    u32::from(depth_domains_requested(unsafe { &(*pass).depth_domains }))
}

// The caller lends a live interface of the named type for this call.
pub(super) unsafe fn borrow<T: Interface>(raw: &Raw) -> &T {
    unsafe { T::from_raw_borrowed(raw) }.expect("live D3D12 interface")
}

// Adopt the output before interpreting the HRESULT so a partial result is released.
#[cfg_attr(test, allow(dead_code))]
fn receive<T: Interface>(call: impl FnOnce(&GUID, *mut Raw) -> HRESULT) -> Option<T> {
    let mut raw = null_mut();
    let status = call(&T::IID, &mut raw);
    let owned = (!raw.is_null()).then(|| unsafe { T::from_raw(raw) });
    if status.is_ok() { owned } else { None }
}

pub(super) fn device_of(child: &ID3D12DeviceChild) -> Option<ID3D12Device> {
    let mut device = None;
    unsafe { child.GetDevice(&mut device) }.ok()?;
    device
}

pub(super) fn owned_by(child: &ID3D12DeviceChild, device: *const c_void) -> bool {
    device_of(child).is_some_and(|owner| owner.as_raw().cast_const() == device)
}

#[cfg_attr(test, allow(dead_code))]
pub(super) fn committed_resource(
    device: &ID3D12Device,
    heap_type: D3D12_HEAP_TYPE,
    desc: &D3D12_RESOURCE_DESC,
    state: D3D12_RESOURCE_STATES,
) -> Raw {
    let heap = D3D12_HEAP_PROPERTIES {
        Type: heap_type,
        ..Default::default()
    };
    let mut resource: Option<ID3D12Resource> = None;
    let status = unsafe {
        device.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_NONE,
            desc,
            state,
            None,
            &mut resource,
        )
    };
    if status.is_err() {
        return null_mut();
    }
    resource.map_or(null_mut(), Interface::into_raw)
}

#[cfg_attr(test, allow(dead_code))]
pub(super) fn texture_desc(width: u32, height: u32, format: DXGI_FORMAT) -> D3D12_RESOURCE_DESC {
    D3D12_RESOURCE_DESC {
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
        ..Default::default()
    }
}

fn transition(
    list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
    if before != after {
        let barrier = crate::d3d11_d3d12::transition_barrier(resource, before, after);
        unsafe { list.ResourceBarrier(&[barrier]) };
    }
}

#[cfg_attr(test, allow(dead_code))]
pub(super) unsafe extern "C" fn rsf_core_d3d12_transition(
    list: Raw,
    resource: Raw,
    before: u32,
    after: u32,
) {
    if before == after {
        return;
    }
    unsafe {
        transition(
            borrow(&list),
            borrow(&resource),
            D3D12_RESOURCE_STATES(before as i32),
            D3D12_RESOURCE_STATES(after as i32),
        );
    }
}

pub(super) unsafe extern "C" fn rsf_core_d3d12_copy_transitioned(
    list: Raw,
    destination: Raw,
    destination_before: u32,
    destination_after: u32,
    source: Raw,
    source_before: u32,
    source_after: u32,
) {
    let list: &ID3D12GraphicsCommandList = unsafe { borrow(&list) };
    let source: &ID3D12Resource = unsafe { borrow(&source) };
    let destination: &ID3D12Resource = unsafe { borrow(&destination) };
    let src = D3D12_RESOURCE_STATE_COPY_SOURCE;
    let dst = D3D12_RESOURCE_STATE_COPY_DEST;
    transition(
        list,
        source,
        D3D12_RESOURCE_STATES(source_before as i32),
        src,
    );
    transition(
        list,
        destination,
        D3D12_RESOURCE_STATES(destination_before as i32),
        dst,
    );
    unsafe { list.CopyResource(destination, source) };
    transition(
        list,
        destination,
        dst,
        D3D12_RESOURCE_STATES(destination_after as i32),
    );
    transition(
        list,
        source,
        src,
        D3D12_RESOURCE_STATES(source_after as i32),
    );
}

#[cfg(not(test))]
pub(super) unsafe extern "C" fn rsf_core_d3d12_create_uav_texture(
    device: Raw,
    width: u32,
    height: u32,
    format: u32,
) -> Raw {
    let mut desc = texture_desc(width, height, DXGI_FORMAT(format as i32));
    desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    committed_resource(
        unsafe { borrow(&device) },
        D3D12_HEAP_TYPE_DEFAULT,
        &desc,
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
    )
}

#[cfg(not(test))]
pub(super) mod fg;
// Called only by sr12's native platform, which unit tests do not build.
#[cfg(not(test))]
pub(super) mod sr;
