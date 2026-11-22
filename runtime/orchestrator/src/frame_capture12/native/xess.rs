// SPDX-License-Identifier: MIT
use super::{Handle, readback_buffer, resource_field, transition};
use crate::xess_fg_bridge::session_records_abi::{
    rsf_xess_bridge_resource_desc, rsf_xess_capture_desc,
};
use std::{ffi::c_void, ptr};
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::{IUnknown, Interface};

const _: () = {
    assert!(size_of::<D3D12_PLACED_SUBRESOURCE_FOOTPRINT>() == 32);
    assert!(std::mem::offset_of!(D3D12_PLACED_SUBRESOURCE_FOOTPRINT, Footprint) == 8);
    assert!(std::mem::offset_of!(D3D12_SUBRESOURCE_FOOTPRINT, RowPitch) == 16);
};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_d3d12_capture_describe(
    source: Handle,
    output: *mut rsf_xess_capture_desc,
) {
    let desc = unsafe {
        ID3D12Resource::from_raw_borrowed(&source)
            .unwrap()
            .GetDesc()
    };
    unsafe {
        output.write(rsf_xess_capture_desc {
            width: desc.Width,
            height: desc.Height,
            dimension: desc.Dimension.0 as u32,
            format: desc.Format.0 as u32,
            depth_or_array_size: u32::from(desc.DepthOrArraySize),
            mip_levels: u32::from(desc.MipLevels),
            sample_count: desc.SampleDesc.Count,
        });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_d3d12_capture_footprint(
    source: Handle,
    footprint: *mut c_void,
    bytes: *mut u64,
    output: *mut Handle,
) -> i32 {
    let source = unsafe { ID3D12Resource::from_raw_borrowed(&source).unwrap() };
    let mut device: Option<ID3D12Device> = None;
    if unsafe { source.GetDevice(&mut device) }.is_err() {
        return -1;
    }
    let Some(device) = device else { return -1 };
    let desc = unsafe { source.GetDesc() };
    unsafe {
        device.GetCopyableFootprints(
            &desc,
            0,
            1,
            0,
            Some(footprint.cast()),
            None,
            None,
            Some(bytes),
        );
        output.write(device.into_raw());
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_d3d12_capture_readback(device: Handle, bytes: u64) -> Handle {
    unsafe { ID3D12Device::from_raw_borrowed(&device) }
        .and_then(|device| readback_buffer(device, bytes))
        .map_or(ptr::null_mut(), Interface::into_raw)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_d3d12_capture_release(reference: Handle) {
    if !reference.is_null() {
        drop(unsafe { IUnknown::from_raw(reference) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_d3d12_capture_copy(
    list: Handle,
    source: Handle,
    readback: Handle,
    footprint: *const c_void,
    state: u32,
) {
    let commands = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list).unwrap() };
    let source = unsafe { ID3D12Resource::from_raw_borrowed(&source).unwrap() };
    let readback = unsafe { ID3D12Resource::from_raw_borrowed(&readback).unwrap() };
    let incoming = D3D12_RESOURCE_STATES(state as i32);
    let copy = D3D12_RESOURCE_STATE_COPY_SOURCE;
    let from = D3D12_TEXTURE_COPY_LOCATION {
        pResource: resource_field(source),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: 0,
        },
    };
    let to = D3D12_TEXTURE_COPY_LOCATION {
        pResource: resource_field(readback),
        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            PlacedFootprint: unsafe {
                footprint
                    .cast::<D3D12_PLACED_SUBRESOURCE_FOOTPRINT>()
                    .read()
            },
        },
    };
    transition(commands, source, incoming, copy);
    unsafe { commands.CopyTextureRegion(&to, 0, 0, 0, &from, None) };
    transition(commands, source, copy, incoming);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_d3d12_capture_map(
    readback: Handle,
    bytes: u64,
    output: *mut *const u8,
) -> i32 {
    let readback = unsafe { ID3D12Resource::from_raw_borrowed(&readback).unwrap() };
    let mut data = ptr::null_mut();
    let range = D3D12_RANGE {
        Begin: 0,
        End: bytes as usize,
    };
    if unsafe { readback.Map(0, Some(&range), Some(&mut data)) }.is_err() {
        return -1;
    }
    unsafe { output.write(data.cast()) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_d3d12_capture_unmap(readback: Handle) {
    unsafe { super::rsf_fg12_platform_unmap(readback) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_d3d12_present_describe(
    source: Handle,
    output: *mut rsf_xess_bridge_resource_desc,
) {
    let desc = unsafe {
        ID3D12Resource::from_raw_borrowed(&source)
            .unwrap()
            .GetDesc()
    };
    unsafe {
        output.write(rsf_xess_bridge_resource_desc {
            width: desc.Width,
            height: desc.Height,
            format: desc.Format.0 as u32,
        });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_d3d12_present_transition(
    list: Handle,
    resource: Handle,
    from: u32,
    to: u32,
) {
    let commands = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list).unwrap() };
    let resource = unsafe { ID3D12Resource::from_raw_borrowed(&resource).unwrap() };
    transition(
        commands,
        resource,
        D3D12_RESOURCE_STATES(from as i32),
        D3D12_RESOURCE_STATES(to as i32),
    );
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xess_d3d12_present_copy(
    list: Handle,
    target: Handle,
    source: Handle,
) {
    let commands = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list).unwrap() };
    let target = unsafe { ID3D12Resource::from_raw_borrowed(&target).unwrap() };
    let source = unsafe { ID3D12Resource::from_raw_borrowed(&source).unwrap() };
    unsafe { commands.CopyResource(target, source) };
}
