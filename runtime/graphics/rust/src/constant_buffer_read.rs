// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_output::same_device;
use crate::sys::rsf_d3d11_output_is_immediate;
use std::ffi::c_void;
use std::ptr;

type Handle = *mut c_void;

const OK: i32 = 0;
const INVALID: i32 = -1;
const TOO_SMALL: i32 = -3;
const STAGING_FAILED: i32 = -4;
const MAP_FAILED: i32 = -5;
const FOREIGN_DEVICE: i32 = -6;

// COM slots and descriptors verified against the C vtables in Windows SDK 10.0.26100.0 d3d11.h.
/// # Safety
///
/// `object` must be a live COM interface whose vtable has more than `index` slots.
pub unsafe fn slot(object: Handle, index: usize) -> Handle {
    let table = unsafe { *object.cast::<*const Handle>() };
    unsafe { *table.add(index) }
}

#[repr(C)]
#[derive(Default)]
struct BufferDesc {
    byte_width: u32,
    usage: u32,
    bind_flags: u32,
    cpu_access_flags: u32,
    misc_flags: u32,
    structure_stride: u32,
}

#[repr(C)]
#[derive(Default)]
pub struct MappedResource {
    pub data: Handle,
    row_pitch: u32,
    depth_pitch: u32,
}

const _: () = {
    assert!(size_of::<BufferDesc>() == 24);
    assert!(size_of::<MappedResource>() == size_of::<Handle>() + 8);
};

pub const MAP_WRITE_DISCARD: u32 = 4;
const MAP_READ: u32 = 1;
pub const USAGE_DYNAMIC: u32 = 2;
const USAGE_STAGING: u32 = 3;
const BIND_CONSTANT_BUFFER: u32 = 4;
const CPU_ACCESS_WRITE: u32 = 0x10000;
const CPU_ACCESS_READ: u32 = 0x20000;
const RESOURCE_DIMENSION_BUFFER: u32 = 1;

unsafe fn buffer_desc(buffer: Handle) -> BufferDesc {
    type GetDesc = unsafe extern "system" fn(Handle, *mut BufferDesc);
    let get_desc: GetDesc = unsafe { std::mem::transmute(slot(buffer, 10)) };
    let mut desc = BufferDesc::default();
    unsafe { get_desc(buffer, &mut desc) };
    desc
}

unsafe fn create_buffer(device: Handle, desc: &BufferDesc) -> Option<ComRef> {
    type CreateBuffer =
        unsafe extern "system" fn(Handle, *const BufferDesc, Handle, *mut Handle) -> i32;
    let create: CreateBuffer = unsafe { std::mem::transmute(slot(device, 3)) };
    let mut buffer = ptr::null_mut();
    if unsafe { create(device, desc, ptr::null_mut(), &mut buffer) } < 0 {
        return None;
    }
    unsafe { ComRef::from_owned(buffer) }
}

/// # Safety
///
/// `device` must be a live `ID3D11Device`.
pub unsafe fn make_dynamic(device: Handle, bytes: u32) -> Option<ComRef> {
    unsafe {
        create_buffer(
            device,
            &BufferDesc {
                byte_width: bytes,
                usage: USAGE_DYNAMIC,
                bind_flags: BIND_CONSTANT_BUFFER,
                cpu_access_flags: CPU_ACCESS_WRITE,
                ..BufferDesc::default()
            },
        )
    }
}

/// # Safety
///
/// `buffer` must be a live `ID3D11Resource`.
pub unsafe fn resource_buffer_info(buffer: Handle) -> Option<(u32, u32, u32)> {
    type GetType = unsafe extern "system" fn(Handle, *mut u32);
    let get_type: GetType = unsafe { std::mem::transmute(slot(buffer, 7)) };
    let mut dimension = 0;
    unsafe { get_type(buffer, &mut dimension) };
    if dimension != RESOURCE_DIMENSION_BUFFER {
        return None;
    }
    let desc = unsafe { buffer_desc(buffer) };
    Some((desc.byte_width, desc.bind_flags, desc.usage))
}

/// # Safety
///
/// `buffer` must be a live `ID3D11Buffer`.
pub unsafe fn constant_size(buffer: Handle, bytes: u32) -> bool {
    let desc = unsafe { buffer_desc(buffer) };
    desc.byte_width == bytes && desc.bind_flags & BIND_CONSTANT_BUFFER != 0
}

pub type Map = unsafe extern "system" fn(Handle, Handle, u32, u32, u32, *mut MappedResource) -> i32;
pub type Unmap = unsafe extern "system" fn(Handle, Handle, u32);

unsafe fn map(context: Handle, buffer: Handle, kind: u32) -> Option<MappedResource> {
    let map: Map = unsafe { std::mem::transmute(slot(context, 14)) };
    let mut mapped = MappedResource::default();
    (unsafe { map(context, buffer, 0, kind, 0, &mut mapped) } >= 0).then_some(mapped)
}

unsafe fn unmap(context: Handle, buffer: Handle) {
    let unmap: Unmap = unsafe { std::mem::transmute(slot(context, 15)) };
    unsafe { unmap(context, buffer, 0) };
}

/// # Safety
///
/// `context` must be a live `ID3D11DeviceContext`, `buffer` a dynamic buffer of its device and
/// `contents` readable for `bytes` bytes, which must fit the buffer.
pub unsafe fn upload(context: Handle, buffer: Handle, contents: *const c_void, bytes: u32) -> bool {
    let Some(mapped) = (unsafe { map(context, buffer, MAP_WRITE_DISCARD) }) else {
        return false;
    };
    if !mapped.data.is_null() {
        unsafe {
            ptr::copy_nonoverlapping(
                contents.cast::<u8>(),
                mapped.data.cast::<u8>(),
                bytes as usize,
            )
        };
    }
    unsafe { unmap(context, buffer) };
    !mapped.data.is_null()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_read_constant_buffer(
    device: Handle,
    context: Handle,
    buffer: Handle,
    destination: Handle,
    bytes: u32,
) -> i32 {
    if device.is_null()
        || context.is_null()
        || buffer.is_null()
        || destination.is_null()
        || bytes == 0
    {
        return INVALID;
    }
    let Some((width, _, _)) = (unsafe { resource_buffer_info(buffer) }) else {
        return INVALID;
    };
    if !unsafe { same_device(device, buffer) && same_device(device, context) } {
        return FOREIGN_DEVICE;
    }
    if width < bytes {
        return TOO_SMALL;
    }
    // Avoid recording a deferred copy that cannot be followed by a read map.
    if unsafe { rsf_d3d11_output_is_immediate(context) } == 0 {
        return MAP_FAILED;
    }
    let Some(staging) = (unsafe {
        create_buffer(
            device,
            &BufferDesc {
                byte_width: width,
                usage: USAGE_STAGING,
                cpu_access_flags: CPU_ACCESS_READ,
                ..BufferDesc::default()
            },
        )
    }) else {
        return STAGING_FAILED;
    };
    type CopyResource = unsafe extern "system" fn(Handle, Handle, Handle);
    let copy: CopyResource = unsafe { std::mem::transmute(slot(context, 47)) };
    unsafe { copy(context, staging.as_ptr(), buffer) };
    let Some(mapped) = (unsafe { map(context, staging.as_ptr(), MAP_READ) }) else {
        return MAP_FAILED;
    };
    let result = if mapped.data.is_null() {
        MAP_FAILED
    } else {
        unsafe {
            ptr::copy_nonoverlapping(
                mapped.data.cast::<u8>(),
                destination.cast::<u8>(),
                bytes as usize,
            )
        };
        OK
    };
    unsafe { unmap(context, staging.as_ptr()) };
    result
}
