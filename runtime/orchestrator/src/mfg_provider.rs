// SPDX-License-Identifier: MIT
// Provider eligibility and retention adapted from MFG-Unlocker 2b52823e.
use super::{Handle, retain_module};
use std::ffi::c_void;
use std::mem::forget;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};
use windows_sys::Win32::{
    Storage::FileSystem::{VS_FFI_SIGNATURE, VS_FIXEDFILEINFO},
    System::LibraryLoader::{GetModuleFileNameW, GetProcAddress},
};

// Plain (not dllimport) references, as the C++ provider policy made them: version.lib resolves
// them, and the dinput8 proxy, which forwards version.dll itself, resolves them to its own
// forwarders. The windows-sys bindings import them through raw-dylib members inside this archive,
// which define the forwarders' names a second time.
unsafe extern "system" {
    fn GetFileVersionInfoSizeW(path: *const u16, handle: *mut u32) -> u32;
    fn GetFileVersionInfoW(path: *const u16, handle: u32, bytes: u32, data: *mut c_void) -> i32;
    fn VerQueryValueW(
        block: *const c_void,
        sub_block: *const u16,
        buffer: *mut *mut c_void,
        bytes: *mut u32,
    ) -> i32;
}

struct Version {
    major: u16,
    minor: u16,
    build: u16,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Profile {
    Unsupported = 0,
    Early = 1,
    LegacySlot11 = 2,
    LegacySlot10 = 3,
    LegacySlot9 = 4,
    V3109 = 5,
    V310916 = 6,
}

impl Version {
    const fn profile(&self) -> Profile {
        if self.major != 310 {
            return Profile::Unsupported;
        }
        match (self.minor, self.build) {
            (1 | 3, 0) | (2, 0 | 1) => Profile::Early,
            (4, 0) | (5, 0 | 2 | 3) => Profile::LegacySlot11,
            (6, 0) => Profile::LegacySlot10,
            (7, 0 | 128 | 129) | (8, 0) => Profile::LegacySlot9,
            (9, 0 | 1) => Profile::V3109,
            (9, 16) => Profile::V310916,
            _ => Profile::Unsupported,
        }
    }
}

pub(super) fn implementation(module: Handle) -> bool {
    !module.is_null()
        && unsafe {
            GetProcAddress(
                module,
                c"NVSDK_NGX_D3D12_PopulateDeviceParameters_Impl"
                    .as_ptr()
                    .cast(),
            )
            .is_some()
                && GetProcAddress(module, c"NVSDK_NGX_DirectSR_Create".as_ptr().cast()).is_none()
        }
}

// The native caller lends a NUL-terminated module path.
unsafe fn version(path: *const u16) -> Option<Version> {
    if path.is_null() || unsafe { *path } == 0 {
        return None;
    }
    let mut ignored = 0;
    let bytes = unsafe { GetFileVersionInfoSizeW(path, &mut ignored) };
    if bytes == 0 {
        return None;
    }
    // DWORD alignment also covers the fixed file information returned inside
    // this buffer by VerQueryValueW.
    let units = (bytes as usize).div_ceil(size_of::<u32>());
    let mut data = Vec::<u32>::new();
    data.try_reserve_exact(units).ok()?;
    data.resize(units, 0);
    if unsafe { GetFileVersionInfoW(path, 0, bytes, data.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    let mut fixed = null_mut();
    let mut fixed_bytes = 0;
    if unsafe {
        VerQueryValueW(
            data.as_ptr().cast(),
            [b'\\' as u16, 0].as_ptr(),
            &mut fixed,
            &mut fixed_bytes,
        )
    } == 0
        || fixed.is_null()
        || (fixed_bytes as usize) < size_of::<VS_FIXEDFILEINFO>()
    {
        return None;
    }
    let fixed = unsafe { fixed.cast::<VS_FIXEDFILEINFO>().read_unaligned() };
    (fixed.dwSignature == VS_FFI_SIGNATURE as u32).then_some(Version {
        major: (fixed.dwFileVersionMS >> 16) as u16,
        minor: fixed.dwFileVersionMS as u16,
        build: (fixed.dwFileVersionLS >> 16) as u16,
    })
}

// The midpoint publisher maps this receipt to its pinned payload profile.
pub(super) unsafe fn provider_profile(path: *const u16) -> u32 {
    unsafe { version(path) }.map_or(Profile::Unsupported, |value| value.profile()) as u32
}

pub(super) unsafe fn supported_provider(module: Handle, path: *const u16) -> u32 {
    u32::from(
        implementation(module) && unsafe { provider_profile(path) } != Profile::Unsupported as u32,
    )
}

// Each occupied slot owns one process-lifetime loader reference. Slots never
// clear, so the base address cannot be reused by a different provider.
static RETAINED: [AtomicPtr<std::ffi::c_void>; 64] = [const { AtomicPtr::new(null_mut()) }; 64];

#[inline(never)]
fn retain_supported_provider(module: Handle) -> bool {
    let Some(retained) = retain_module(module) else {
        return false;
    };
    if retained.as_ptr() != module || !implementation(module) {
        return false;
    }
    let mut path = [0u16; 32768];
    if module_path(module, &mut path).is_none()
        || unsafe { supported_provider(module, path.as_ptr()) } == 0
    {
        return false;
    }
    for slot in &RETAINED {
        match slot.compare_exchange(null_mut(), module, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => {
                forget(retained);
                return true;
            }
            Err(existing) if existing == module => break,
            Err(_) => {}
        }
    }
    // A racing owner or a full table leaves our temporary reference local.
    // The native patch caller retains the provider for the current operation.
    true
}

// The caller holds a loader reference while reading and lending this path.
pub(super) fn module_path(module: Handle, path: &mut [u16; 32768]) -> Option<usize> {
    let length =
        unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32) } as usize;
    (length != 0 && length < path.len()).then_some(length)
}

// The NGX gate requires a process-lifetime reference before it scans or
// publishes provider code.
pub(super) fn retained_provider(module: Handle) -> bool {
    if module.is_null() {
        return false;
    }
    for slot in &RETAINED {
        let cached = slot.load(Ordering::Acquire);
        if cached == module {
            return implementation(module);
        }
        if cached.is_null() {
            break;
        }
    }
    retain_supported_provider(module)
}
