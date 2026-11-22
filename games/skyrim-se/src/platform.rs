// SPDX-License-Identifier: MIT
//! Checked process platform and native exception boundary for the Skyrim adapter.
use rsf_game_sys::rsf_game_host_services;
pub(crate) use rsf_game_sys::{
    rsf_d3d11_creation_scope as CreationScope, rsf_d3d11_native_chain as NativeChain,
};
use std::ffi::{CStr, OsString, c_char, c_void};
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleFileNameW,
    GetModuleHandleExW, GetModuleHandleW, GetProcAddress,
};
use windows_sys::Win32::System::Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery};

pub(crate) const PLATFORM_VERSION: u32 = 4;

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
    pub(crate) struct_size: u32,
    pub(crate) version: u32,
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

impl CreationCall {
    pub(crate) fn valid(&self) -> bool {
        self.struct_size as usize >= size_of::<Self>() && self.version == PLATFORM_VERSION
    }
}

#[repr(C)]
pub(crate) struct Platform {
    _struct_size: u32,
    _version: u32,
    pub(crate) d3d11_host_abi: u32,
    _reserved: u32,
    read: unsafe extern "C" fn(address: usize, out: *mut c_void, size: usize) -> bool,
    pub(crate) create_detour: *const c_void,
    log: unsafe extern "C" fn(host: *const rsf_game_host_services, message: *const c_char),
    creation_begin:
        unsafe extern "C" fn(service: usize, user: *mut c_void, scope: *mut CreationScope) -> i32,
    creation_end: unsafe extern "C" fn(
        service: usize,
        user: *mut c_void,
        scope: *const CreationScope,
        outputs: *const NativeChain,
    ) -> i32,
    render_allowed: unsafe extern "C" fn() -> bool,
}

#[cfg(not(test))]
mod native {
    use super::*;
    use winapi::shared::dxgi::{DXGI_SWAP_CHAIN_DESC, IDXGIAdapter, IDXGISwapChain};
    use winapi::shared::minwindef::{HMODULE, UINT};
    use winapi::shared::ntdef::HRESULT;
    use winapi::um::d3d11::{ID3D11Device, ID3D11DeviceContext};
    use winapi::um::d3dcommon::{D3D_DRIVER_TYPE, D3D_FEATURE_LEVEL};

    unsafe extern "system" {
        fn rsf_skyrim_create_detour(
            adapter: *mut IDXGIAdapter,
            driver: D3D_DRIVER_TYPE,
            software: HMODULE,
            flags: UINT,
            levels: *const D3D_FEATURE_LEVEL,
            level_count: UINT,
            sdk: UINT,
            description: *const DXGI_SWAP_CHAIN_DESC,
            chain: *mut *mut IDXGISwapChain,
            device: *mut *mut ID3D11Device,
            selected: *mut D3D_FEATURE_LEVEL,
            context: *mut *mut ID3D11DeviceContext,
        ) -> HRESULT;
    }

    unsafe extern "C" {
        fn rsf_skyrim_guarded_read(address: usize, out: *mut c_void, size: usize) -> bool;
        fn rsf_skyrim_creation_begin_guarded(
            service: usize,
            user: *mut c_void,
            scope: *mut CreationScope,
        ) -> i32;
        fn rsf_skyrim_creation_end_guarded(
            service: usize,
            user: *mut c_void,
            scope: *const CreationScope,
            outputs: *const NativeChain,
        ) -> i32;
    }

    unsafe extern "C" fn log(host: *const rsf_game_host_services, message: *const c_char) {
        unsafe { crate::render_native::log_host(host, message) };
    }

    pub(crate) static NATIVE_PLATFORM: Platform = Platform {
        _struct_size: size_of::<Platform>() as u32,
        _version: PLATFORM_VERSION,
        d3d11_host_abi: rsf_game_sys::RSF_D3D11_HOST_ABI_VERSION,
        _reserved: 0,
        read: rsf_skyrim_guarded_read,
        create_detour: rsf_skyrim_create_detour as *const c_void,
        log,
        creation_begin: rsf_skyrim_creation_begin_guarded,
        creation_end: rsf_skyrim_creation_end_guarded,
        render_allowed: crate::compatibility::render_allowed,
    };
}

#[cfg(not(test))]
pub(crate) use native::NATIVE_PLATFORM;

const _: () = {
    assert!(size_of::<MemoryRegion>() == 32);
    assert!(size_of::<CreationScope>() == 24);
    assert!(size_of::<NativeChain>() == 32);
    assert!(size_of::<CreationCall>() == 136);
    assert!(std::mem::offset_of!(CreationCall, native_result) == 48);
    assert!(std::mem::offset_of!(CreationCall, owner) == 56);
    assert!(std::mem::offset_of!(CreationCall, scope) == 80);
    assert!(std::mem::offset_of!(CreationCall, outputs) == 104);
    assert!(size_of::<Platform>() == 16 + 6 * size_of::<usize>());
    assert!(std::mem::offset_of!(Platform, create_detour) == 16 + size_of::<usize>());
};

// The platform record is immutable and lives for the process.
unsafe impl Sync for Platform {}

static PLATFORM: AtomicPtr<Platform> = AtomicPtr::new(ptr::null_mut());

/// Accepts the adapter's table when its size and version match this crate.
///
/// # Safety
/// `platform` is null or points at a table readable for at least its leading header that, when
/// accepted, lives for the process.
pub(crate) unsafe fn inspect(platform: *const Platform) -> Option<&'static Platform> {
    if platform.is_null() {
        return None;
    }
    // Only the leading header is readable from a short or foreign table.
    let header = platform.cast::<u32>();
    if (unsafe { header.read() } as usize) < size_of::<Platform>()
        || unsafe { header.add(1).read() } != PLATFORM_VERSION
    {
        return None;
    }
    Some(unsafe { &*platform })
}

/// Publishes the checked table for the cold creation detour.
///
/// # Safety
/// Same contract as `inspect`.
pub(crate) unsafe fn register(platform: *const Platform) -> Option<&'static Platform> {
    let published = PLATFORM.load(Ordering::Acquire);
    if !published.is_null() {
        return (published == platform.cast_mut()).then(|| unsafe { &*published });
    }
    let table = unsafe { inspect(platform) }?;
    match PLATFORM.compare_exchange(
        ptr::null_mut(),
        platform.cast_mut(),
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => Some(table),
        Err(published) => (published == platform.cast_mut()).then_some(table),
    }
}

pub(crate) fn get() -> Option<&'static Platform> {
    // Only `register` stores, and only a process-lived table.
    unsafe { PLATFORM.load(Ordering::Acquire).as_ref() }
}

/// `name` as a NUL-terminated UTF-16 string.
pub(crate) fn wide(name: &str) -> Vec<u16> {
    name.encode_utf16().chain(Some(0)).collect()
}

pub(crate) fn module_path(module: usize, path: &mut [u16]) -> usize {
    let capacity = u32::try_from(path.len()).unwrap_or(u32::MAX);
    unsafe { GetModuleFileNameW(module as _, path.as_mut_ptr(), capacity) as usize }
}

pub(super) fn hex_sha256(path: &Path) -> Option<[u8; 64]> {
    let digest = rsf_windows::sha256_file(path)?;
    let mut hex = [0; 64];
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for (byte, pair) in digest.into_iter().zip(hex.chunks_exact_mut(2)) {
        pair[0] = DIGITS[usize::from(byte >> 4)];
        pair[1] = DIGITS[usize::from(byte & 0x0f)];
    }
    Some(hex)
}

impl Platform {
    pub(crate) fn main_module(&self) -> usize {
        unsafe { GetModuleHandleW(ptr::null()) as usize }
    }

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
        unsafe { (self.read)(address, out.as_mut_ptr().cast(), out.len()) }
    }

    pub(crate) fn read_u32(&self, address: usize) -> Option<u32> {
        let mut value = [0; 4];
        self.read(address, &mut value)
            .then(|| u32::from_le_bytes(value))
    }

    pub(crate) fn read_usize(&self, address: usize) -> Option<usize> {
        let mut value = [0; size_of::<usize>()];
        self.read(address, &mut value)
            .then(|| usize::from_le_bytes(value))
    }

    /// Reads `expected.len()` bytes at `address` and compares them.
    pub(crate) fn matches(&self, address: usize, expected: &[u8]) -> bool {
        let mut actual = [0; 128];
        let Some(actual) = actual.get_mut(..expected.len()) else {
            return false;
        };
        self.read(address, actual) && actual == expected
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
        let mut path = [0u16; 32_768];
        let length = module_path(module, &mut path);
        if length == 0 || length >= path.len() {
            return None;
        }
        let path = OsString::from_wide(&path[..length]);
        hex_sha256(Path::new(&path))
    }

    pub(crate) fn pin_module(&self, address: usize) -> usize {
        let mut pinned = ptr::null_mut();
        let result = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                address as *const u16,
                &mut pinned,
            )
        };
        if result == 0 { 0 } else { pinned as usize }
    }

    pub(crate) fn last_error(&self) -> u32 {
        unsafe { GetLastError() }
    }

    pub(crate) fn set_last_error(&self, error: u32) {
        unsafe { SetLastError(error) };
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

    pub(crate) fn log(&self, host: &rsf_game_host_services, message: &CStr) {
        unsafe { (self.log)(host, message.as_ptr()) }
    }

    /// Logs formatted text; it never contains an interior NUL.
    pub(crate) fn log_text(&self, host: &rsf_game_host_services, message: &str) {
        let mut text = Vec::with_capacity(message.len() + 1);
        text.extend_from_slice(message.as_bytes());
        text.push(0);
        if let Ok(message) = CStr::from_bytes_with_nul(&text) {
            self.log(host, message);
        }
    }

    pub(crate) fn creation_begin(
        &self,
        service: usize,
        user: *mut c_void,
        scope: &mut CreationScope,
    ) -> i32 {
        unsafe { (self.creation_begin)(service, user, scope) }
    }

    pub(crate) fn creation_end(
        &self,
        service: usize,
        user: *mut c_void,
        scope: &CreationScope,
        outputs: Option<&NativeChain>,
    ) -> i32 {
        let outputs = outputs.map_or(ptr::null(), ptr::from_ref);
        unsafe { (self.creation_end)(service, user, scope, outputs) }
    }

    pub(crate) fn render_allowed(&self) -> bool {
        unsafe { (self.render_allowed)() }
    }
}

#[cfg(test)]
mod tests;
