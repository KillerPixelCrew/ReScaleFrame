// SPDX-License-Identifier: MIT
//! Native leaves lent by `native_creation.cpp`, mirrored by hand from `native_creation_platform.h`
//! (bindgen-cli 0.72.1 was not available). Both sides assert sizes and offsets. The leaves carry
//! fault-guarded reads, Win32 module, memory and last-error calls, MinHook, exception guards around
//! host and Runtime callbacks, the Community Shaders gate and the Steam facade; every Skyrim
//! address, expected byte and decision stays in this crate. The archive imports no native symbol.
use rsf_game_sys::rsf_game_host_services;
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

pub(crate) const PLATFORM_VERSION: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct MemoryRegion {
    pub(crate) base: usize,
    pub(crate) allocation_base: usize,
    pub(crate) size: usize,
    pub(crate) state: u32,
    pub(crate) protect: u32,
}

/// `rsf_d3d11_creation_scope` from `d3d11_host.h`.
#[allow(dead_code)] // The Runtime reads the header, window and token.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct CreationScope {
    pub(crate) struct_size: u32,
    pub(crate) abi_version: u32,
    pub(crate) window: *mut c_void,
    pub(crate) token: u64,
}

impl CreationScope {
    pub(crate) fn new(abi_version: u32) -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            abi_version,
            window: ptr::null_mut(),
            token: 0,
        }
    }
}

/// `rsf_d3d11_native_chain` from `d3d11_host.h`.
#[allow(dead_code)] // The Runtime reads every field.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct NativeChain {
    pub(crate) struct_size: u32,
    pub(crate) abi_version: u32,
    pub(crate) device: *mut c_void,
    pub(crate) immediate_context: *mut c_void,
    pub(crate) swapchain: *mut *mut c_void,
}

impl NativeChain {
    pub(crate) fn new(abi_version: u32) -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            abi_version,
            device: ptr::null_mut(),
            immediate_context: ptr::null_mut(),
            swapchain: ptr::null_mut(),
        }
    }
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

pub(crate) type ColdOwnerFn = unsafe extern "C" fn() -> bool;

#[repr(C)]
pub(crate) struct Platform {
    _struct_size: u32,
    _version: u32,
    pub(crate) d3d11_host_abi: u32,
    _reserved: u32,
    main_module: unsafe extern "C" fn() -> usize,
    module_handle: unsafe extern "C" fn(name: *const u16) -> usize,
    export_address: unsafe extern "C" fn(module: usize, name: *const c_char) -> usize,
    read: unsafe extern "C" fn(address: usize, out: *mut c_void, size: usize) -> bool,
    query: unsafe extern "C" fn(address: usize, out: *mut MemoryRegion) -> bool,
    module_sha256: unsafe extern "C" fn(module: usize, digest: *mut c_char) -> bool,
    pin_module: unsafe extern "C" fn(address: usize) -> usize,
    last_error: unsafe extern "C" fn() -> u32,
    set_last_error: unsafe extern "C" fn(error: u32),
    hook_create: unsafe extern "C" fn(
        target: usize,
        detour: *const c_void,
        original: *mut *mut c_void,
    ) -> bool,
    hook_enable: unsafe extern "C" fn(target: usize) -> bool,
    hook_disable: unsafe extern "C" fn(target: usize) -> bool,
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
    steam_prepare:
        unsafe extern "C" fn(host: *const rsf_game_host_services, cold_owner: ColdOwnerFn) -> bool,
}

const _: () = {
    assert!(size_of::<MemoryRegion>() == 32);
    assert!(size_of::<CreationScope>() == 24);
    assert!(size_of::<NativeChain>() == 32);
    assert!(size_of::<CreationCall>() == 136);
    assert!(std::mem::offset_of!(CreationCall, native_result) == 48);
    assert!(std::mem::offset_of!(CreationCall, owner) == 56);
    assert!(std::mem::offset_of!(CreationCall, scope) == 80);
    assert!(std::mem::offset_of!(CreationCall, outputs) == 104);
    assert!(size_of::<Platform>() == 16 + 18 * size_of::<usize>());
    assert!(std::mem::offset_of!(Platform, create_detour) == 16 + 12 * size_of::<usize>());
    assert!(std::mem::offset_of!(Platform, steam_prepare) == 16 + 17 * size_of::<usize>());
};

// The native table is a constant with static storage in the adapter DLL.
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
    let table = unsafe { inspect(platform) }?;
    PLATFORM.store(platform.cast_mut(), Ordering::Release);
    Some(table)
}

pub(crate) fn get() -> Option<&'static Platform> {
    // Only `register` stores, and only a process-lived table.
    unsafe { PLATFORM.load(Ordering::Acquire).as_ref() }
}

/// `name` as a NUL-terminated UTF-16 string.
pub(crate) fn wide(name: &str) -> Vec<u16> {
    name.encode_utf16().chain(Some(0)).collect()
}

impl Platform {
    pub(crate) fn main_module(&self) -> usize {
        unsafe { (self.main_module)() }
    }

    pub(crate) fn module_handle(&self, name: &str) -> usize {
        let name = wide(name);
        unsafe { (self.module_handle)(name.as_ptr()) }
    }

    pub(crate) fn export_address(&self, module: usize, name: &CStr) -> usize {
        unsafe { (self.export_address)(module, name.as_ptr()) }
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
        let mut region = MemoryRegion::default();
        unsafe { (self.query)(address, &mut region) }.then_some(region)
    }

    pub(crate) fn module_sha256(&self, module: usize) -> Option<[u8; 64]> {
        let mut digest = [0u8; 64];
        unsafe { (self.module_sha256)(module, digest.as_mut_ptr().cast()) }.then_some(digest)
    }

    pub(crate) fn pin_module(&self, address: usize) -> usize {
        unsafe { (self.pin_module)(address) }
    }

    pub(crate) fn last_error(&self) -> u32 {
        unsafe { (self.last_error)() }
    }

    pub(crate) fn set_last_error(&self, error: u32) {
        unsafe { (self.set_last_error)(error) }
    }

    /// Creates a disabled hook and returns its non-null trampoline.
    pub(crate) fn hook_create(&self, target: usize, detour: *const c_void) -> Option<*mut c_void> {
        let mut original = ptr::null_mut();
        let created = unsafe { (self.hook_create)(target, detour, &mut original) };
        (created && !original.is_null()).then_some(original)
    }

    pub(crate) fn hook_enable(&self, target: usize) -> bool {
        unsafe { (self.hook_enable)(target) }
    }

    pub(crate) fn hook_disable(&self, target: usize) -> bool {
        unsafe { (self.hook_disable)(target) }
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

    pub(crate) fn steam_prepare(&self, host: &rsf_game_host_services, cold: ColdOwnerFn) -> bool {
        unsafe { (self.steam_prepare)(host, cold) }
    }
}

#[cfg(test)]
mod tests;
