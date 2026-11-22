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

type Handle = *mut c_void;
const OK: i32 = 0;
const NOT_READY: i32 = -8;
const WRONG_API: i32 = -12;

unsafe extern "C" {
    fn rsf_dlss_verify_runtime_signature(path: *const u16) -> u32;
    fn rsf_streamline_host_graphics(host: Handle, out: *mut Graphics) -> i32;
    fn rsf_streamline_host_module(host: Handle) -> Handle;
}

/// Whether the interposer at `path` is a signed NVIDIA module. The check itself is the vendor
/// header's, which links into the native leaf.
pub(crate) unsafe fn verify_signature(path: *const u16) -> bool {
    unsafe { rsf_dlss_verify_runtime_signature(path) != 0 }
}

pub(crate) unsafe extern "C" fn rsf_dlss_sr_load_sdk(path: *const u16, setup: &Setup) -> Handle {
    unsafe {
        load_interposer(
            path,
            setup.require_signature != 0,
            crate::input_dump::Log::new(setup.log, setup.log_user),
        )
    }
}

/// Loads `sl.interposer.dll` from an absolute path after the signature check when one is required.
/// This puts a signed NVIDIA module into a game process, so the path alone never decides which
/// module answers: every Streamline owner uses the same fixed search.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_load_interposer(
    path: *const u16,
    require_signature: u32,
    log: crate::render_abi::rsf_backend_log_fn,
    user: Handle,
) -> Handle {
    unsafe {
        load_interposer(
            path,
            require_signature != 0,
            crate::input_dump::Log::new(log, user),
        )
    }
}

unsafe fn load_interposer(
    path: *const u16,
    require_signature: bool,
    log: crate::input_dump::Log,
) -> Handle {
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
    if require_signature && !unsafe { verify_signature(path) } {
        log.say(&format!(
            "interposer at {} could not be verified as signed",
            path_text()
        ));
        return null_mut();
    }
    let (module, error) = unsafe { crate::sdk_module::load_absolute_library(path) };
    if module.is_null() {
        log.say(&format!(
            "could not load {} (Windows error {error})",
            path_text()
        ));
    }
    module
}

pub(crate) unsafe extern "C" fn rsf_dlss_sr_host_device(host: Handle) -> Handle {
    let mut graphics = Graphics {
        struct_size: size_of::<Graphics>() as u32,
        ..Graphics::EMPTY
    };
    if unsafe { rsf_streamline_host_graphics(host, &mut graphics) } != OK {
        return null_mut();
    }
    graphics.native_device
}

pub(crate) unsafe extern "C" fn rsf_dlss_sr_shared_module(
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

pub(crate) unsafe extern "C" fn rsf_dlss_sr_d3d11_luid(device: Handle, out: &mut u64) -> i32 {
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

pub(crate) unsafe extern "C" fn rsf_dlss_sr_d3d12_luid(device: Handle, out: &mut u64) -> i32 {
    *out = unsafe { d3d12_luid(device) };
    OK
}

pub(crate) unsafe extern "C" fn rsf_dlss_sr_validate_d3d12(
    device: Handle,
    frame: *const Frame,
) -> i32 {
    let result = unsafe { rsf_core_validate_sr_frame(frame) };
    if result != OK {
        return result;
    }
    crate::d3d12_sr::validate_resources(device, unsafe { &*frame })
}
