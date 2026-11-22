// SPDX-License-Identifier: MIT
use super::{d3d12_luid, luid_bits};
use crate::d3d11_d3d12::borrow;
use crate::d3d11_present_abi::rsf_streamline_graphics as Graphics;
use crate::gl_pipeline_abi::rsf_dlss_setup as Setup;
use crate::render_abi::rsf_sr_frame as Frame;
use crate::sr_session::rsf_core_validate_sr_frame;
use std::ffi::c_void;
use std::ptr::null_mut;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::core::{Interface, PCWSTR};
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};

type Handle = *mut c_void;
const OK: i32 = 0;
const INVALID: i32 = -1;
const NOT_READY: i32 = -8;
const WRONG_API: i32 = -12;

unsafe extern "C" {
    fn rsf_dlss_sdk_verify_signature(path: *const u16) -> bool;
    fn rsf_streamline_host_graphics(host: Handle, out: *mut Graphics) -> i32;
    fn rsf_streamline_host_module(host: Handle) -> Handle;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_sr_load_sdk(path: *const u16, setup: &Setup) -> Handle {
    let log = crate::input_dump::Log::new(setup.log, setup.log_user);
    let path_text = || {
        if path.is_null() {
            "(null)".to_owned()
        } else {
            unsafe { PCWSTR(path).to_string() }.unwrap_or_else(|_| "(invalid UTF-16)".to_owned())
        }
    };
    if unsafe { crate::streamline_policy::rsf_streamline_absolute_path(path) } == 0 {
        log.say(&format!("interposer path is not absolute: {}", path_text()));
        return null_mut();
    }
    if setup.require_signature != 0 && !unsafe { rsf_dlss_sdk_verify_signature(path) } {
        log.say(&format!(
            "interposer at {} could not be verified as signed",
            path_text()
        ));
        return null_mut();
    }
    let module = unsafe {
        LoadLibraryExW(
            path,
            null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if module.is_null() {
        let error = unsafe { GetLastError() };
        log.say(&format!(
            "could not load {} (Windows error {error})",
            path_text()
        ));
    }
    module
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_sr_host_device(host: Handle) -> Handle {
    let mut graphics = Graphics {
        struct_size: size_of::<Graphics>() as u32,
        ..Graphics::EMPTY
    };
    if unsafe { rsf_streamline_host_graphics(host, &mut graphics) } != OK {
        return null_mut();
    }
    graphics.native_device
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_sr_shared_module(
    device: Handle,
    host: Handle,
    out: &mut Handle,
) -> i32 {
    if unsafe { rsf_dlss_sr_host_device(host) } != device {
        return WRONG_API;
    }
    *out = unsafe { rsf_streamline_host_module(host) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_sr_d3d11_luid(device: Handle, out: &mut u64) -> i32 {
    let device = unsafe { borrow::<windows::Win32::Graphics::Direct3D11::ID3D11Device>(&device) };
    let description = device
        .cast::<IDXGIDevice>()
        .and_then(|dxgi| unsafe { dxgi.GetAdapter()?.GetDesc() });
    let Ok(description) = description else {
        return NOT_READY;
    };
    *out = luid_bits(description.AdapterLuid);
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_sr_d3d12_luid(device: Handle, out: &mut u64) -> i32 {
    *out = unsafe { d3d12_luid(device) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_sr_validate_d3d12(device: Handle, frame: *const Frame) -> i32 {
    let result = unsafe { rsf_core_validate_sr_frame(frame) };
    if result != OK {
        return result;
    }
    crate::d3d12_sr::validate_resources(device, unsafe { &*frame })
}
