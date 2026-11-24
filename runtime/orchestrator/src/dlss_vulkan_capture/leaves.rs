// SPDX-License-Identifier: MIT
//! The present capture's native state, setup, frame-capture and file leaves, formerly part of
//! native_fg_dlss_vulkan_capture.cpp. The DLSS-G fixture replaces these leaves; the owner in
//! dlss_vulkan_capture.rs makes the Vulkan, WSI and registration calls itself.
use super::{Handle, Platform};
use crate::frame_capture_vk::{
    Owner as Capture, rsf_core_capture_vk_create, rsf_core_capture_vk_destroy,
    rsf_core_capture_vk_enabled, rsf_core_capture_vk_flush, rsf_core_capture_vk_initialize,
    rsf_core_capture_vk_pending, rsf_core_capture_vk_provider, rsf_core_capture_vk_record,
    rsf_core_capture_vk_wants,
};
use crate::sys::rsf_fgvk_setup as Setup;
use crate::sys::rsf_srvk_image as Image;
use crate::vulkan_capture_commands::Commands;
use ash::vk::{self, Handle as _};
use rsf_game_sys::backend_result::{
    INIT_FAILED, INVALID_ARGUMENT, MISSING_ENTRY_POINT, NOT_READY, OK,
};
use rsf_game_sys::{RSF_FRAME_FLAG_RESET, rsf_frame_record as Record};
use rsf_streamline_vk::sys::rsf_sl_present_capture_registration as Registration;
use std::ffi::{CStr, OsString, c_char};
use std::fs::OpenOptions;
use std::io::Write as _;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::PathBuf;
use std::ptr;
use windows_sys::Win32::System::Environment::GetEnvironmentVariableW;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

const SHARED_SUBMISSION: u32 = 0;

/// `rsfSlSetPresentCapture` of the corrected common plugin.
pub(crate) type Register = unsafe extern "C" fn(*const Registration) -> i32;

/// The capture's native handle.
pub(crate) struct Native {
    pub(super) setup: Setup,
    pub(super) register: Register,
    pub(super) capture: *mut Capture,
    directory: PathBuf,
    pub(super) commands: Option<Commands>,
}

impl Native {
    /// A capture native without file output, with the given registration
    /// entry. The capture owner is created but not initialized.
    pub(crate) fn new(setup: &Setup, register: Register) -> Box<Self> {
        Box::new(Self {
            setup: *setup,
            register,
            capture: rsf_core_capture_vk_create(SHARED_SUBMISSION),
            directory: PathBuf::new(),
            commands: None,
        })
    }

    fn load_commands(&mut self) -> bool {
        self.commands = Commands::load(&self.setup.graphics);
        self.commands.is_some()
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        unsafe { rsf_core_capture_vk_destroy(self.capture) };
    }
}

pub(super) fn native<'a>(native: Handle) -> &'a Native {
    unsafe { &*native.cast::<Native>() }
}

fn environment(name: &CStr) -> Option<OsString> {
    let name: Vec<u16> = name.to_str().ok()?.encode_utf16().chain([0]).collect();
    let mut value = vec![0u16; 32768];
    let length = unsafe { GetEnvironmentVariableW(name.as_ptr(), value.as_mut_ptr(), 32768) };
    (length != 0 && length < 32768).then(|| OsString::from_wide(&value[..length as usize]))
}

unsafe extern "C" fn open(setup: *const Setup, out: *mut Handle) -> i32 {
    unsafe { out.write(ptr::null_mut()) };
    let setup = unsafe { &*setup };
    let graphics = &setup.graphics;
    let capture = rsf_core_capture_vk_create(SHARED_SUBMISSION);
    let enabled = unsafe { rsf_core_capture_vk_enabled(capture) } != 0;
    unsafe { rsf_core_capture_vk_destroy(capture) };
    if !enabled {
        return OK;
    }
    let Some(directory) = environment(c"RSF_VULKAN_FRAME_CAPTURE_DIR") else {
        return INVALID_ARGUMENT;
    };
    if graphics.runtime_directory_utf8.is_null() {
        return INVALID_ARGUMENT;
    }
    let runtime = unsafe { CStr::from_ptr(graphics.runtime_directory_utf8) };
    let Ok(runtime) = runtime.to_str() else {
        return INVALID_ARGUMENT;
    };
    let common: Vec<u16> = PathBuf::from(runtime)
        .join("sl.common.dll")
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect();
    let module = unsafe { GetModuleHandleW(common.as_ptr()) };
    if module.is_null() {
        return NOT_READY;
    }
    let Some(register) =
        (unsafe { GetProcAddress(module, c"rsfSlSetPresentCapture".as_ptr().cast()) })
    else {
        return MISSING_ENTRY_POINT;
    };
    let register: Register = unsafe { std::mem::transmute(register) };
    let mut next = Native::new(setup, register);
    next.directory = PathBuf::from(directory);
    if !next.load_commands() {
        return MISSING_ENTRY_POINT;
    }
    let Some(instance_proc) = graphics.instance_proc else {
        return MISSING_ENTRY_POINT;
    };
    let name = c"vkGetPhysicalDeviceMemoryProperties";
    let Some(properties) = (unsafe { instance_proc(graphics.instance, name.as_ptr()) }) else {
        return MISSING_ENTRY_POINT;
    };
    let properties: vk::PFN_vkGetPhysicalDeviceMemoryProperties =
        unsafe { std::mem::transmute(properties) };
    let mut memory = vk::PhysicalDeviceMemoryProperties::default();
    unsafe {
        properties(
            vk::PhysicalDevice::from_raw(graphics.physical_device as u64),
            &mut memory,
        )
    };
    unsafe { rsf_core_capture_vk_provider(next.capture, 0, 1, 1) };
    if unsafe {
        rsf_core_capture_vk_initialize(next.capture, graphics.device, graphics.device_proc, &memory)
    } == 0
    {
        return INIT_FAILED;
    }
    unsafe { out.write(Box::into_raw(next).cast()) };
    OK
}

unsafe extern "C" fn wants(handle: Handle, frame: u64) -> u32 {
    u32::from(unsafe { rsf_core_capture_vk_wants(native(handle).capture, frame) } != 0)
}

unsafe extern "C" fn readbacks(handle: Handle) -> u64 {
    unsafe { rsf_core_capture_vk_pending(native(handle).capture) }
}

unsafe extern "C" fn record(
    handle: Handle,
    commands: Handle,
    image: *const Image,
    identity: *const Record,
    role: *const c_char,
) -> u32 {
    let reset = unsafe { (*identity).flags } & RSF_FRAME_FLAG_RESET;
    let recorded = unsafe {
        rsf_core_capture_vk_record(
            native(handle).capture,
            commands,
            image,
            identity,
            role,
            reset,
            0,
            ptr::null_mut(),
            commands as u64,
        )
    };
    u32::from(recorded != 0)
}

unsafe extern "C" fn flush(handle: Handle) -> u32 {
    let flushed = unsafe { rsf_core_capture_vk_flush(native(handle).capture, 0, ptr::null_mut()) };
    u32::from(flushed != 0)
}

fn append_bytes(handle: Handle, file: &str, bytes: &[u8]) {
    let path = native(handle).directory.join(file);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(bytes);
    }
}

unsafe extern "C" fn failure(handle: Handle, source: u64, reason: *const c_char, result: i32) {
    let reason = unsafe { CStr::from_ptr(reason) }.to_string_lossy();
    let line = format!("{source} {reason} {result}\n");
    append_bytes(handle, "dlss-present-capture-errors.log", line.as_bytes());
}

unsafe extern "C" fn append(handle: Handle, file: *const c_char, line: *const u8, length: usize) {
    let file = unsafe { CStr::from_ptr(file) }.to_string_lossy();
    let line = unsafe { std::slice::from_raw_parts(line, length) };
    append_bytes(handle, &file, line);
}

/// The production capture and file leaves. The DLSS-G fixture copies the table and replaces
/// open and every entry.
pub(crate) static LEAVES: Platform = Platform {
    open,
    wants,
    readbacks,
    record,
    flush,
    failure,
    append,
};

/// A capture native with the fixture's registration entry and no file output
/// or capture initialization, for the DLSS-G present-capture fixture.
#[cfg(feature = "test-fixtures")]
pub(crate) unsafe fn fixture_native(setup: *const Setup, register: Register) -> Handle {
    let Some(setup) = (unsafe { setup.as_ref() }) else {
        return ptr::null_mut();
    };
    let mut native = Native::new(setup, register);
    if !native.load_commands() {
        return ptr::null_mut();
    }
    Box::into_raw(native).cast()
}
