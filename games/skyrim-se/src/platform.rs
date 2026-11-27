// SPDX-License-Identifier: MIT
//! Process platform and native exception boundary for the Skyrim adapter.
use rsf_game_sys::rsf_game_host_services;
use rsf_game_sys::win32::module_path;
pub(crate) use rsf_game_sys::{
    rsf_d3d11_creation_scope as CreationScope, rsf_d3d11_native_chain as NativeChain,
};
use std::ffi::{CStr, OsString, c_void};
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::ptr;
use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct MemoryRegion {
    pub(crate) base: usize,
    pub(crate) allocation_base: usize,
    pub(crate) size: usize,
    pub(crate) state: u32,
    pub(crate) protect: u32,
}

/// `rsf_skyrim_creation_call`: one guarded creation call on the detour's stack.
#[repr(C)]
pub(crate) struct CreationCall {
    pub(crate) caller: usize,
    pub(crate) description: *const c_void,
    pub(crate) swapchain: *mut *mut c_void,
    pub(crate) device: *mut *mut c_void,
    pub(crate) context: *mut *mut c_void,
    pub(crate) native_result: i32,
    pub(crate) returned: u8,
    pub(crate) begun: u8,
    pub(crate) matched: u8,
    _reserved: u8,
    pub(crate) owner: usize,
    pub(crate) window: *mut c_void,
    pub(crate) outer: *mut c_void,
    pub(crate) scope: CreationScope,
    pub(crate) outputs: NativeChain,
}

/// The process platform: Win32 queries plus the native fault and exception boundary.
pub(crate) struct Platform;

mod native {
    use super::{CreationScope, NativeChain};
    use std::ffi::c_void;

    unsafe extern "system" {
        // The D3D11CreateDeviceAndSwapChain detour; only its address is taken.
        pub(super) fn rsf_skyrim_create_detour();
    }

    unsafe extern "C" {
        pub(super) fn rsf_skyrim_creation_begin_guarded(
            service: usize,
            user: *mut c_void,
            scope: *mut CreationScope,
        ) -> i32;
        pub(super) fn rsf_skyrim_creation_end_guarded(
            service: usize,
            user: *mut c_void,
            scope: *const CreationScope,
            outputs: *const NativeChain,
        ) -> i32;
    }
}

const _: () = {
    assert!(size_of::<MemoryRegion>() == 32);
    assert!(size_of::<CreationScope>() == 24);
    assert!(size_of::<NativeChain>() == 32);
    assert!(size_of::<CreationCall>() == 128);
    assert!(std::mem::offset_of!(CreationCall, native_result) == 40);
    assert!(std::mem::offset_of!(CreationCall, owner) == 48);
    assert!(std::mem::offset_of!(CreationCall, scope) == 72);
    assert!(std::mem::offset_of!(CreationCall, outputs) == 96);
};

pub(crate) use rsf_game_sys::ffi_util::wide;

pub(super) fn hex_sha256(path: &Path) -> Option<[u8; 64]> {
    rsf_windows::sha256_file_hex(path)?.first_chunk().copied()
}

impl Platform {
    pub(crate) fn module_handle(&self, name: &str) -> usize {
        let name = wide(name);
        unsafe { GetModuleHandleW(name.as_ptr()) as usize }
    }

    pub(crate) fn export_address(&self, module: usize, name: &CStr) -> usize {
        if module == 0 {
            return 0;
        }
        unsafe { GetProcAddress(module as _, name.as_ptr().cast()) }
            .map_or(0, |address| address as usize)
    }

    pub(crate) fn read(&self, address: usize, out: &mut [u8]) -> bool {
        unsafe { rsf_game_sys::win32::read_process(address, out.as_mut_ptr().cast(), out.len()) }
    }

    pub(crate) fn read_u32(&self, address: usize) -> Option<u32> {
        rsf_game_sys::win32::read(address)
    }

    pub(crate) fn read_usize(&self, address: usize) -> Option<usize> {
        rsf_game_sys::win32::read(address)
    }

    pub(crate) fn query(&self, address: usize) -> Option<MemoryRegion> {
        let mut region = MEMORY_BASIC_INFORMATION::default();
        let read = unsafe {
            VirtualQuery(
                address as *const c_void,
                &mut region,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        (read != 0).then_some(MemoryRegion {
            base: region.BaseAddress as usize,
            allocation_base: region.AllocationBase as usize,
            size: region.RegionSize,
            state: region.State,
            protect: region.Protect,
        })
    }

    pub(crate) fn module_sha256(&self, module: usize) -> Option<[u8; 64]> {
        let path = unsafe { module_path(module as _) }?;
        hex_sha256(Path::new(&OsString::from_wide(path.units())))
    }

    pub(crate) fn last_error(&self) -> u32 {
        unsafe { GetLastError() }
    }

    pub(crate) fn set_last_error(&self, error: u32) {
        unsafe { SetLastError(error) };
    }

    pub(crate) fn log(&self, host: &rsf_game_host_services, message: &CStr) {
        unsafe { crate::render_native::log_host(host, message.as_ptr()) };
    }

    pub(crate) fn log_text(&self, host: &rsf_game_host_services, message: &str) {
        rsf_game_sys::host_log(host, message);
    }

    pub(crate) fn creation_begin(
        &self,
        service: usize,
        user: *mut c_void,
        scope: &mut CreationScope,
    ) -> i32 {
        unsafe { native::rsf_skyrim_creation_begin_guarded(service, user, scope) }
    }

    pub(crate) fn creation_end(
        &self,
        service: usize,
        user: *mut c_void,
        scope: &CreationScope,
        outputs: Option<&NativeChain>,
    ) -> i32 {
        let outputs = outputs.map_or(ptr::null(), ptr::from_ref);
        unsafe { native::rsf_skyrim_creation_end_guarded(service, user, scope, outputs) }
    }

    pub(crate) fn create_detour(&self) -> *const c_void {
        native::rsf_skyrim_create_detour as *const c_void
    }

    pub(crate) fn render_allowed(&self) -> bool {
        crate::compatibility::render_allowed()
    }
}

/// The calls that reach the game image and MinHook.
#[cfg(not(test))]
impl Platform {
    pub(crate) fn main_module(&self) -> usize {
        unsafe { GetModuleHandleW(ptr::null()) as usize }
    }

    pub(crate) fn pin_module(&self, address: usize) -> usize {
        rsf_game_sys::win32::pin_module(address as *const c_void)
            .map_or(0, |module| module as usize)
    }

    pub(crate) fn hook_create(&self, target: usize, detour: *const c_void) -> Option<*mut c_void> {
        let mut original = ptr::null_mut();
        let created = unsafe {
            rsf_windows::minhook::create(target as *mut c_void, detour.cast_mut(), &mut original)
        };
        (created && !original.is_null()).then_some(original)
    }

    pub(crate) fn hook_enable(&self, target: usize) -> bool {
        unsafe { rsf_windows::minhook::enable_idempotent(target as *mut c_void) }
    }

    pub(crate) fn hook_disable(&self, target: usize) -> bool {
        unsafe { rsf_windows::minhook::disable(target as *mut c_void) }
    }
}

/// Unit tests refuse them, so they never reach the game image or MinHook.
#[cfg(test)]
impl Platform {
    pub(crate) fn main_module(&self) -> usize {
        0
    }

    pub(crate) fn pin_module(&self, _: usize) -> usize {
        0
    }

    pub(crate) fn hook_create(&self, _: usize, _: *const c_void) -> Option<*mut c_void> {
        None
    }

    pub(crate) fn hook_enable(&self, _: usize) -> bool {
        false
    }

    pub(crate) fn hook_disable(&self, _: usize) -> bool {
        false
    }
}
