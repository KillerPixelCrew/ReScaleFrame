// SPDX-License-Identifier: MIT
mod abi;
#[cfg(windows)]
mod dx12;
#[cfg(windows)]
mod fingerprint;
// The orchestrator's stateless FidelityFX provider-version policy.
#[path = "../../../../orchestrator/src/ffx_version.rs"]
mod ffx_version;
// The FSR 4 DX12 bridge owner for Vulkan SR. Both the Runtime and the
// standalone VulkanStoryFsr4.dll already link this archive.
#[path = "../../../../vulkan/fsr4/owner.rs"]
mod vulkan_bridge;
// The standalone VulkanStoryFsr3.dll links its owner with rustc, not this
// archive. Only its CPU fixture runs here.
#[cfg(test)]
#[path = "../../../../vulkan/fsr3/owner.rs"]
mod vulkan_fsr3_bridge;

use abi::*;
use core::ffi::{CStr, c_void};
use core::ptr::{self, NonNull};
use std::sync::{Arc, Mutex, PoisonError, RwLock, Weak};

struct Build {
    version: &'static CStr,
    capability_rva: usize,
    file_size: u32,
    expected: [u8; 16],
    digest: [u8; 32],
}

// The third virtual entry after RTTI is the INT8 provider's capability predicate.
// The FP8 predicate and all other shader and device checks stay native.
static BUILDS: [Build; 2] = [
    // AMD SDK 2.3.0 (60f4ea81), vtable entry .rdata+1af36b8.
    Build {
        version: c"4.1.1.2740",
        capability_rva: 0x8d70,
        file_size: 28_761_864,
        expected: [
            0x48, 0x83, 0xec, 0x48, 0x48, 0x8b, 0xc2, 0x48, 0x85, 0xd2, 0x74, 0x54, 0x48, 0x8d,
            0x54, 0x24,
        ],
        digest: [
            0xd0, 0xdc, 0xcc, 0xc7, 0x4a, 0x43, 0xc4, 0x4b, 0xa4, 0x35, 0xb7, 0xa3, 0x69, 0xb4,
            0x56, 0xe0, 0x97, 0x0d, 0x8a, 0x44, 0x64, 0xe4, 0xbd, 0x68, 0x31, 0x19, 0xb3, 0x74,
            0xf2, 0xc9, 0xfb, 0x46,
        ],
    },
    // AMD SDK 2.4.0 (7539ae8b), vtable entry .rdata+3635270. Its native Shader
    // Model 6.6 requirement is also checked before granting a lease.
    Build {
        version: c"4.1.2.4555",
        capability_rva: 0x9960,
        file_size: 57_664_272,
        expected: [
            0x40, 0x53, 0x48, 0x83, 0xec, 0x50, 0x48, 0x8b, 0x05, 0x93, 0x76, 0x63, 0x03, 0x48,
            0x33, 0xc4,
        ],
        digest: [
            0xaa, 0xd4, 0xfc, 0xcc, 0xcb, 0x29, 0x08, 0x4a, 0x65, 0x5f, 0x9a, 0x4f, 0x9f, 0x61,
            0x3b, 0xd2, 0x2c, 0x8c, 0x03, 0xfb, 0x4a, 0x0f, 0x88, 0x40, 0x14, 0x8c, 0xc6, 0x54,
            0x3b, 0x4c, 0xca, 0xce,
        ],
    },
];

struct Lease {
    device: NonNull<c_void>,
    release: unsafe extern "C" fn(*mut c_void),
}

// The retained D3D12 device can be released from any thread. The lease shares
// only its identity and never exposes a borrowed COM object.
unsafe impl Send for Lease {}
unsafe impl Sync for Lease {}

impl Drop for Lease {
    fn drop(&mut self) {
        // Arc's last strong reference is already gone, so the detour forwards.
        unsafe { (self.release)(self.device.as_ptr()) };
    }
}

struct Resident {
    module_address: usize,
    build: &'static Build,
}

struct Registration {
    original: rsf_fsr4_compat_capability_fn,
    device_address: usize,
    lease: Weak<Lease>,
}

// Serialize admission and installation. The detour only takes the registration
// lock, which must be released before calling MinHook or the original SDK entry.
static OWNER: Mutex<Option<Resident>> = Mutex::new(None);
static REGISTRATION: RwLock<Registration> = RwLock::new(Registration {
    original: None,
    device_address: 0,
    lease: Weak::new(),
});

extern "C" fn capability(api: *mut c_void, device: *mut c_void) -> bool {
    let registration = REGISTRATION.read().unwrap_or_else(PoisonError::into_inner);
    // Seeing zero strong references is enough to revoke admission, even if
    // the last owner's COM Release is reentrant or has yet to run.
    if registration.device_address == device.addr() && registration.lease.strong_count() != 0 {
        return true;
    }
    let original = registration.original;
    drop(registration);
    // The trampoline is published before enabling the hook and remains resident.
    original.is_some_and(|call| unsafe { call(api, device) })
}

fn install(
    platform: &rsf_fsr4_compat_platform,
    module: *mut c_void,
    diagnostic: &mut rsf_fsr4_compat_diagnostic,
) -> Option<Resident> {
    let sizes = BUILDS.each_ref().map(|build| build.file_size);
    let mut size = 0;
    let mut digest = [0; 32];
    let read_digest = platform.module_digest?;
    let match_entry = platform.match_entry?;
    let create_hook = platform.create_hook?;
    let enable_hook = platform.enable_hook?;
    // The native file leaf checks size before hashing and writes into these
    // caller-owned outputs. It borrows the still-loaded module for the call.
    let read = unsafe {
        read_digest(
            module,
            sizes.as_ptr(),
            sizes.len(),
            &mut size,
            digest.as_mut_ptr(),
        )
    };
    let build = if read {
        BUILDS
            .iter()
            .find(|build| build.file_size == size && build.digest == digest)
    } else {
        None
    };
    let target = build.and_then(|build| {
        NonNull::new(unsafe {
            match_entry(
                module,
                build.capability_rva,
                build.expected.as_ptr(),
                build.expected.len(),
            )
        })
        .map(|target| (build, target))
    });
    let Some((build, target)) = target else {
        diagnostic.refusal =
            c"FSR4 INT8 compatibility refused: SDK binary fingerprint or capability bytes differ"
                .as_ptr();
        return None;
    };

    let Some(original) = (unsafe { create_hook(target.as_ptr(), Some(capability)) }) else {
        diagnostic.refusal =
            c"FSR4 INT8 compatibility refused: resident hook creation failed".as_ptr();
        return None;
    };
    REGISTRATION
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .original = Some(original);
    let pinned = unsafe { enable_hook(target.as_ptr(), Some(capability)) };
    if pinned.is_null() {
        REGISTRATION
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .original = None;
        diagnostic.refusal =
            c"FSR4 INT8 compatibility refused: resident hook preparation failed".as_ptr();
        return None;
    }
    Some(Resident {
        module_address: pinned.addr(),
        build,
    })
}

fn acquire(
    platform: &rsf_fsr4_compat_platform,
    module: *mut c_void,
    adapter: *mut c_void,
    device: NonNull<c_void>,
    diagnostic: &mut rsf_fsr4_compat_diagnostic,
) -> Option<Arc<Lease>> {
    let vendor = unsafe { (platform.adapter_vendor?)(adapter) };
    // Supported AMD devices use the SDK's native selection.
    if !matches!(vendor, 0x10de | 0x8086) {
        return None;
    }
    if !unsafe { (platform.device_supports_int8?)(device.as_ptr()) } {
        diagnostic.refusal =
            c"FSR4 INT8 compatibility refused: Shader Model 6.6 and wave operations are required"
                .as_ptr();
        return None;
    }
    let retain = platform.retain_device?;
    let release = platform.release_device?;
    let device_address = device.as_ptr().addr();

    let mut owner = OWNER.lock().unwrap_or_else(PoisonError::into_inner);
    let same_module = owner
        .as_ref()
        .is_some_and(|resident| resident.module_address == module.addr());
    {
        let registration = REGISTRATION.read().unwrap_or_else(PoisonError::into_inner);
        if same_module
            && registration.device_address == device_address
            && let Some(existing) = registration.lease.upgrade()
        {
            return Some(existing);
        }
        if registration.lease.strong_count() != 0 {
            diagnostic.refusal =
                c"FSR4 INT8 compatibility refused: another runtime/device owns the capability hook"
                    .as_ptr();
            return None;
        }
    }
    if owner.is_some() && !same_module {
        diagnostic.refusal =
            c"FSR4 INT8 compatibility refused: another SDK module owns the resident hook".as_ptr();
        return None;
    }
    if owner.is_none() {
        *owner = Some(install(platform, module, diagnostic)?);
    }
    let resident = owner.as_ref()?;
    // The caller owns the device during this call. Keep a reference until the
    // last facade drops, so its address cannot be recycled during admission.
    unsafe { retain(device.as_ptr()) };
    let lease = Arc::new(Lease { device, release });
    {
        let mut registration = REGISTRATION.write().unwrap_or_else(PoisonError::into_inner);
        registration.device_address = device_address;
        registration.lease = Arc::downgrade(&lease);
    }
    diagnostic.sdk_version = resident.build.version.as_ptr();
    diagnostic.capability_rva = resident.build.capability_rva;
    Some(lease)
}

/// # Safety
/// The platform header and diagnostic must be readable and writable respectively.
/// A full platform supplies valid callbacks and the native callers keep the module,
/// matched adapter and device alive for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_fsr4_compat_acquire(
    platform: *const rsf_fsr4_compat_platform,
    module: *mut c_void,
    adapter: *mut c_void,
    device: *mut c_void,
    diagnostic: *mut rsf_fsr4_compat_diagnostic,
) -> *const rsf_fsr4_compat_lease {
    let Some(diagnostic) = (unsafe { diagnostic.as_mut() }) else {
        return ptr::null();
    };
    *diagnostic = rsf_fsr4_compat_diagnostic {
        refusal: ptr::null(),
        sdk_version: ptr::null(),
        capability_rva: 0,
    };
    let Some(device) = NonNull::new(device) else {
        return ptr::null();
    };
    if platform.is_null() || module.is_null() || adapter.is_null() {
        return ptr::null();
    }
    let size = unsafe { (&raw const (*platform).struct_size).read() };
    if (size as usize) < size_of::<rsf_fsr4_compat_platform>() {
        return ptr::null();
    }
    let platform = unsafe { &*platform };
    if platform.abi_version != RSF_FSR4_COMPAT_PLATFORM_ABI_VERSION {
        return ptr::null();
    }
    acquire(platform, module, adapter, device, diagnostic)
        .map_or(ptr::null(), |lease| Arc::into_raw(lease).cast())
}

/// # Safety
/// Borrowing and callback requirements are the same as for acquire. The log
/// callback consumes a borrowed message during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_fsr4_compat_acquire_logged(
    platform: *const rsf_fsr4_compat_platform,
    module: *mut c_void,
    adapter: *mut c_void,
    device: *mut c_void,
    log: rsf_fsr4_compat_log_fn,
    user: *mut c_void,
) -> *const rsf_fsr4_compat_lease {
    let mut diagnostic = rsf_fsr4_compat_diagnostic {
        refusal: ptr::null(),
        sdk_version: ptr::null(),
        capability_rva: 0,
    };
    let lease =
        unsafe { rsf_fsr4_compat_acquire(platform, module, adapter, device, &mut diagnostic) };
    if let Some(log) = log {
        if lease.is_null() && !diagnostic.refusal.is_null() {
            unsafe { log(user, diagnostic.refusal) };
        } else if !diagnostic.sdk_version.is_null() {
            let version = unsafe { CStr::from_ptr(diagnostic.sdk_version) }.to_string_lossy();
            let message = format!(
                "FSR4 INT8 compatibility active: SDK {version}, device-scoped predicate RVA {:#x}; real adapter identity preserved\0",
                diagnostic.capability_rva
            );
            unsafe { log(user, message.as_ptr().cast()) };
        }
    }
    lease
}

/// # Safety
/// A non-null reference must come from acquire and be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_fsr4_compat_release(lease: *const rsf_fsr4_compat_lease) {
    if !lease.is_null() {
        drop(unsafe { Arc::from_raw(lease.cast::<Lease>()) });
    }
}
