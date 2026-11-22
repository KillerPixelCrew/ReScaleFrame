// SPDX-License-Identifier: MIT

use crate::sys::*;
use std::ffi::c_void;
use std::ptr;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_CREATE_FACTORY_FLAGS,
    DXGI_ERROR_NOT_FOUND, IDXGIFactory4,
};

type Create = unsafe extern "C" fn(*const rsf_fsr4fg_create, *mut *mut c_void) -> i32;
type Probe = unsafe extern "C" fn(u64, *const u16, *mut rsf_fsr4fg_info) -> i32;
type Start = unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32;
type Present = unsafe extern "C" fn(
    *mut c_void,
    *const rsf_fsr4fg_frame,
    *mut rsf_fsr4fg_present_status,
) -> i32;
const ABI: u32 = RSF_FSR4FG_BRIDGE_ABI_VERSION;
const INVALID: i32 = RSF_FSR4FG_INVALID_ARGUMENT;
const UNSUPPORTED: i32 = RSF_FSR4FG_NOT_SUPPORTED;

fn setup(luid: u64, directory: *const u16) -> rsf_fsr4fg_create {
    rsf_fsr4fg_create {
        struct_size: size_of::<rsf_fsr4fg_create>() as u32,
        abi_version: ABI,
        adapter_luid: luid,
        runtime_directory: directory,
        width: 640,
        height: 360,
        depth_inverted: 1,
        depth_infinite: 0,
        motion_jittered: 0,
        generated_count: 1,
    }
}

fn info() -> rsf_fsr4fg_info {
    rsf_fsr4fg_info {
        struct_size: size_of::<rsf_fsr4fg_info>() as u32,
        abi_version: ABI,
        adapter_luid: 0,
        provider_version: 0,
        vendor_id: 0,
        max_generated: 0,
    }
}

fn contract(create: Create, probe: Probe, start: Start, present: Present) -> Result<(), i32> {
    let mut owner = ptr::without_provenance_mut(1);
    let mut request = setup(1, ptr::null());
    request.struct_size -= 1;
    request.abi_version = 99;
    if unsafe { create(&request, &mut owner) } != INVALID || !owner.is_null() {
        return Err(10);
    }
    request.struct_size += 1;
    if unsafe { create(&request, &mut owner) } != RSF_FSR4FG_ABI_MISMATCH || !owner.is_null() {
        return Err(11);
    }
    let relative: Vec<_> = "relative\0".encode_utf16().collect();
    request.abi_version = ABI;
    request.runtime_directory = relative.as_ptr();
    if unsafe { create(&request, &mut owner) } != INVALID || !owner.is_null() {
        return Err(12);
    }
    let absolute: Vec<_> = "C:\\fixture\0".encode_utf16().collect();
    request.runtime_directory = absolute.as_ptr();
    request.generated_count = 2;
    if unsafe { create(&request, &mut owner) } != INVALID || !owner.is_null() {
        return Err(13);
    }
    let mut admission = rsf_fsr4fg_info {
        adapter_luid: 8,
        provider_version: 9,
        vendor_id: 10,
        max_generated: 11,
        ..info()
    };
    if unsafe { probe(1, relative.as_ptr(), &mut admission) } != INVALID
        || admission.adapter_luid != 0
        || admission.provider_version != 0
        || admission.vendor_id != 0
        || admission.max_generated != 0
    {
        return Err(14);
    }
    if unsafe { start(ptr::null_mut(), ptr::without_provenance_mut(1)) } != INVALID {
        return Err(15);
    }
    let mut status = rsf_fsr4fg_present_status {
        struct_size: size_of::<rsf_fsr4fg_present_status>() as u32,
        abi_version: ABI,
        frames_presented: 23,
        fg_enabled: 1,
        result: 0,
        terminal: 1,
        generated_callbacks: 17,
    };
    if unsafe { present(ptr::null_mut(), ptr::null(), &mut status) } != INVALID
        || status.result != INVALID
        || status.frames_presented != 0
        || status.fg_enabled != 0
        || status.terminal != 0
        || status.generated_callbacks != 0
    {
        return Err(17);
    }
    Ok(())
}

fn devices(create: Create, probe: Probe, directory: *const u16) -> Result<(), i32> {
    let factory: IDXGIFactory4 =
        unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.map_err(|_| 20)?;
    let mut queried = 0;
    for index in 0.. {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(_) => return Err(21),
        };
        let desc = unsafe { adapter.GetDesc1() }.map_err(|_| 22)?;
        drop(adapter);
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let luid = u64::from(desc.AdapterLuid.LowPart)
            | (u64::from(desc.AdapterLuid.HighPart as u32) << 32);
        let mut queried_info = info();
        let result = unsafe { probe(luid, directory, &mut queried_info) };
        println!(
            "{{\"device_query\":true,\"vendor\":{},\"device\":{},\"adapter_luid\":{luid},\"probe\":{result},\"provider_version\":{},\"maximum_generated\":{},\"hwnd_supplied\":false}}",
            desc.VendorId, desc.DeviceId, queried_info.provider_version, queried_info.max_generated,
        );
        if result == UNSUPPORTED {
            let mut owner = ptr::null_mut();
            if unsafe { create(&setup(luid, directory), &mut owner) } != UNSUPPORTED
                || !owner.is_null()
                || queried_info.provider_version != 0
            {
                return Err(23);
            }
        } else if result != 0
            || queried_info.adapter_luid != luid
            || queried_info.provider_version == 0
            || queried_info.max_generated != 1
        {
            return Err(24);
        }
        queried += 1;
    }
    if queried == 0 {
        return Err(25);
    }
    println!(
        "{{\"admission_passed\":true,\"rendering_executed\":false,\"presentation_replaced\":false}}"
    );
    Ok(())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4_fg_bridge_fixture(
    argc: i32,
    argv: *const *const u16,
    create: Create,
    probe: Probe,
    start: Start,
    present: Present,
) -> i32 {
    if let Err(code) = contract(create, probe, start, present) {
        return code;
    }
    if argc == 1 {
        println!("{{\"contract_passed\":true,\"device_query\":false}}");
        return 0;
    }
    if argc != 3 {
        return 16;
    }
    let device = unsafe { argv.add(1).read() };
    for (index, expected) in "--device\0".encode_utf16().enumerate() {
        if unsafe { device.add(index).read() } != expected {
            return 16;
        }
    }
    devices(create, probe, unsafe { argv.add(2).read() })
        .err()
        .unwrap_or(0)
}
