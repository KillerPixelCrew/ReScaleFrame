// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::sr_session::has_capacity;
use crate::sys::RSF_SRVK_ABI_VERSION;
use crate::sys::*;
use crate::vulkan_fg_copy::SHADER_READ;
use crate::vulkan_fg_frames::PresentSource;
use crate::vulkan_ownership::Claim;
use crate::vulkan_presentation::ProviderOperation;
use rsf_game_sys::rsf_frame_record;
use std::ffi::c_void;
use std::ptr::{self, addr_of, addr_of_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use ::rsf_vk_xess_fg::bridge_abi::{
    RSF_XESS_BRIDGE_ABI_MISMATCH, RSF_XESS_BRIDGE_INVALID_ARGUMENT,
};
use rsf_game_sys::backend_result::{
    ABI_MISMATCH, FEATURE_FAILED, INIT_FAILED, INVALID_ARGUMENT as INVALID, NOT_COMPILED,
    NOT_READY, NOT_SUPPORTED, OK,
};

/// `VK_ERROR_INITIALIZATION_FAILED`, the WSI entries' failure.
const WSI_INITIALIZATION_FAILED: i32 = -3;

const DLSS: u32 = 1;
const FFX: u32 = 3;
const FSR4: u32 = 4;
const XESS: u32 = 5;

type Handle = *mut c_void;

/// One frame-generation provider behind the public `rsf_fgvk_*` entries. Factory entries return
/// backend results, operational entries the provider's result for [`Owner::result`], and the WSI
/// entries the provider's VkResult unchanged.
struct Provider {
    create: unsafe fn(
        *const rsf_fgvk_setup,
        *const rsf_fgvk_host_options,
        u32,
        *mut Handle,
        *mut u64,
    ) -> i32,
    enable: unsafe fn(Handle, u32, u32) -> i32,
    images: unsafe fn(Handle, *mut u32, *mut u64) -> i32,
    acquire: unsafe fn(Handle, u64, u64, u64, *mut u32) -> i32,
    /// None selects the shader-read layout.
    image_layout: Option<unsafe fn(Handle, *mut u32) -> i32>,
    /// None selects a one-frame limit.
    max_generated: Option<unsafe fn(Handle, *mut u32) -> i32>,
    prepare: unsafe fn(Handle, Handle, *const rsf_fgvk_frame) -> i32,
    hudless: unsafe fn(Handle, Handle, *const rsf_fgvk_frame) -> i32,
    submitted: unsafe fn(Handle, *const Handle, u32),
    present: unsafe fn(Handle, *const c_void) -> i32,
    present_source: unsafe fn(Handle, *const c_void, *const rsf_frame_record) -> i32,
    status: unsafe fn(Handle, *mut rsf_fgvk_status) -> i32,
    drain: unsafe fn(Handle) -> i32,
    destroy: unsafe fn(Handle) -> i32,
    latency_options: Option<unsafe fn(Handle, u32, u32) -> i32>,
    latency_sleep: Option<unsafe fn(Handle, u32) -> i32>,
    latency_marker: Option<unsafe fn(Handle, u32, u32) -> i32>,
    latency_status: Option<unsafe fn(Handle, *mut rsf_fgvk_latency_status) -> i32>,
}

/// The providers of a build; None reports NOT_COMPILED.
struct Platform {
    dlss: Option<&'static Provider>,
    ffx: Option<&'static Provider>,
    fsr4: Option<&'static Provider>,
    xess: Option<&'static Provider>,
}

// The provider modules. The dispatch tests substitute fakes with the same entry names.
#[cfg(all(not(test), feature = "xess-fg-vk"))]
use crate::vulkan_xess_fg as xess;
#[cfg(not(test))]
use crate::{vulkan_dlss_fg as dlss, vulkan_ffx_fg as ffx, vulkan_fsr4_fg as fsr4};
#[cfg(test)]
use dispatch_tests::{dlss, ffx, fsr4, xess};

/// Writes the provider's owner through `out` whatever the result, as each factory does.
macro_rules! created {
    ($out:expr, |$owner:ident| $call:expr) => {{
        let mut $owner = ptr::null_mut();
        let result = $call;
        unsafe { $out.write($owner.cast()) };
        result
    }};
}

unsafe fn dlss_status(owner: Handle, status: &mut rsf_dlssgvk_status) -> i32 {
    status.struct_size = size_of::<rsf_dlssgvk_status>() as u32;
    status.abi_version = RSF_DLSSGVK_ABI_VERSION;
    unsafe { dlss::rsf_dlssgvk_get_status(owner.cast(), status) }
}

static DLSS_PROVIDER: Provider = Provider {
    create: |setup, _, _, out, chain| {
        created!(out, |owner| unsafe {
            dlss::rsf_dlssgvk_create(setup, &mut owner, chain)
        })
    },
    enable: |owner, enabled, count| unsafe {
        dlss::rsf_dlssgvk_enable(owner.cast(), enabled, count)
    },
    images: |owner, count, images| unsafe { dlss::rsf_dlssgvk_images(owner.cast(), count, images) },
    acquire: |owner, timeout, semaphore, fence, index| unsafe {
        dlss::rsf_dlssgvk_acquire(owner.cast(), timeout, semaphore, fence, index)
    },
    image_layout: Some(|owner, layout| unsafe {
        dlss::rsf_dlssgvk_image_layout(owner.cast(), layout)
    }),
    max_generated: Some(|owner, count| {
        let mut status = rsf_dlssgvk_status::default();
        let result = unsafe { dlss_status(owner, &mut status) };
        if result == OK {
            unsafe { count.write(status.max_generated_frames) };
        }
        result
    }),
    prepare: |owner, commands, frame| unsafe {
        dlss::rsf_dlssgvk_prepare(owner.cast(), commands, frame)
    },
    hudless: |owner, commands, frame| unsafe {
        dlss::rsf_dlssgvk_hudless(owner.cast(), commands, frame)
    },
    submitted: |owner, commands, count| unsafe {
        dlss::rsf_dlssgvk_submitted(owner.cast(), commands, count)
    },
    present: |owner, info| unsafe { dlss::rsf_dlssgvk_present(owner.cast(), info) },
    present_source: |owner, info, source| unsafe {
        dlss::rsf_dlssgvk_present_source(owner.cast(), info, source)
    },
    status: |owner, out| {
        let mut status = rsf_dlssgvk_status::default();
        let result = unsafe { dlss_status(owner, &mut status) };
        if result == OK {
            unsafe { out.write(status.presentation) };
        }
        result
    },
    drain: |owner| unsafe { dlss::rsf_dlssgvk_drain(owner.cast()) },
    destroy: |owner| unsafe { dlss::rsf_dlssgvk_destroy(owner.cast()) },
    latency_options: None,
    latency_sleep: None,
    latency_marker: None,
    latency_status: None,
};

static FFX_PROVIDER: Provider = Provider {
    create: |setup, _, _, out, chain| {
        created!(out, |owner| unsafe {
            ffx::rsf_ffxgvk_create(setup, &mut owner, chain)
        })
    },
    // FidelityFX 3 generates one frame, so the requested count is not forwarded.
    enable: |owner, enabled, _| unsafe { ffx::rsf_ffxgvk_enable(owner.cast(), enabled) },
    images: |owner, count, images| unsafe { ffx::rsf_ffxgvk_images(owner.cast(), count, images) },
    acquire: |owner, timeout, semaphore, fence, index| unsafe {
        ffx::rsf_ffxgvk_acquire(owner.cast(), timeout, semaphore, fence, index)
    },
    image_layout: None,
    max_generated: None,
    prepare: |owner, commands, frame| unsafe {
        ffx::rsf_ffxgvk_prepare(owner.cast(), commands, frame)
    },
    hudless: |owner, commands, frame| unsafe {
        ffx::rsf_ffxgvk_hudless(owner.cast(), commands, frame)
    },
    submitted: |owner, commands, count| unsafe {
        ffx::rsf_ffxgvk_submitted(owner.cast(), commands, count)
    },
    present: |owner, info| unsafe { ffx::rsf_ffxgvk_present(owner.cast(), info) },
    present_source: |owner, info, source| unsafe {
        ffx::rsf_ffxgvk_present_source(owner.cast(), info, source)
    },
    status: |owner, status| unsafe { ffx::rsf_ffxgvk_get_status(owner.cast(), status) },
    drain: |owner| unsafe { ffx::rsf_ffxgvk_drain(owner.cast()) },
    destroy: |owner| unsafe { ffx::rsf_ffxgvk_destroy(owner.cast()) },
    latency_options: None,
    latency_sleep: None,
    latency_marker: None,
    latency_status: None,
};

static FSR4_PROVIDER: Provider = Provider {
    create: |setup, host, count, out, chain| {
        // Admission requires the host record for FSR4.
        let host = unsafe { &*host };
        let options = fsr4::Options {
            struct_size: size_of::<fsr4::Options>() as u32,
            abi_version: 1,
            window: host.window,
            timeline_enabled: host.timeline_enabled,
            generated_count: count,
        };
        let result = created!(out, |owner| unsafe {
            fsr4::rsf_fsr4gvk_create(setup, (&raw const options).cast(), &mut owner, chain)
        });
        rsf_core_fgvk_provider_result(FSR4, result)
    },
    enable: |owner, enabled, count| unsafe {
        fsr4::rsf_fsr4gvk_enable_options(owner.cast(), enabled, count)
    },
    images: |owner, count, images| unsafe { fsr4::rsf_fsr4gvk_images(owner.cast(), count, images) },
    acquire: |owner, timeout, semaphore, fence, index| unsafe {
        fsr4::rsf_fsr4gvk_acquire(owner.cast(), timeout, semaphore, fence, index)
    },
    image_layout: None,
    max_generated: Some(|owner, count| {
        unsafe { count.write(fsr4::rsf_fsr4gvk_max_generated(owner.cast())) };
        OK
    }),
    prepare: |owner, commands, frame| unsafe {
        fsr4::rsf_fsr4gvk_prepare(owner.cast(), commands, frame)
    },
    hudless: |owner, commands, frame| unsafe {
        fsr4::rsf_fsr4gvk_hudless(owner.cast(), commands, frame)
    },
    submitted: |owner, commands, count| unsafe {
        fsr4::rsf_fsr4gvk_submitted(owner.cast(), commands, count)
    },
    present: |owner, info| unsafe { fsr4::rsf_fsr4gvk_present(owner.cast(), info) },
    present_source: |owner, info, source| unsafe {
        fsr4::rsf_fsr4gvk_present_source(owner.cast(), info, source)
    },
    status: |owner, status| unsafe { fsr4::rsf_fsr4gvk_status(owner.cast(), status) },
    drain: |owner| unsafe { fsr4::rsf_fsr4gvk_drain(owner.cast()) },
    destroy: |owner| unsafe { fsr4::rsf_fsr4gvk_destroy(owner.cast()) },
    latency_options: None,
    latency_sleep: None,
    latency_marker: None,
    latency_status: None,
};

#[cfg(any(test, feature = "xess-fg-vk"))]
static XESS_PROVIDER: Provider = Provider {
    create: |setup, host, count, out, chain| {
        // Admission requires the host record for XeSS.
        let host = unsafe { &*host };
        let options = rsf_xessgvk_options {
            struct_size: size_of::<rsf_xessgvk_options>() as u32,
            abi_version: 1,
            window: host.window,
            timeline_enabled: host.timeline_enabled,
            generated_count: count,
            intel_runtime_directory: host.intel_runtime_directory,
        };
        let result = created!(out, |owner| unsafe {
            xess::rsf_xessgvk_create(setup, (&raw const options).cast(), &mut owner, chain)
        });
        rsf_core_fgvk_provider_result(XESS, result)
    },
    enable: |owner, enabled, count| unsafe {
        xess::rsf_xessgvk_enable_options(owner.cast(), enabled, count)
    },
    images: |owner, count, images| unsafe { xess::rsf_xessgvk_images(owner.cast(), count, images) },
    acquire: |owner, timeout, semaphore, fence, index| unsafe {
        xess::rsf_xessgvk_acquire(owner.cast(), timeout, semaphore, fence, index)
    },
    image_layout: None,
    max_generated: Some(|owner, count| {
        unsafe { count.write(xess::rsf_xessgvk_max_generated(owner.cast())) };
        OK
    }),
    prepare: |owner, commands, frame| unsafe {
        xess::rsf_xessgvk_prepare(owner.cast(), commands, frame)
    },
    hudless: |owner, commands, frame| unsafe {
        xess::rsf_xessgvk_hudless(owner.cast(), commands, frame)
    },
    submitted: |owner, commands, count| unsafe {
        xess::rsf_xessgvk_submitted(owner.cast(), commands, count)
    },
    present: |owner, info| unsafe { xess::rsf_xessgvk_present(owner.cast(), info) },
    present_source: |owner, info, source| unsafe {
        xess::rsf_xessgvk_present_source(owner.cast(), info, source)
    },
    status: |owner, status| unsafe { xess::rsf_xessgvk_status(owner.cast(), status) },
    drain: |owner| unsafe { xess::rsf_xessgvk_drain(owner.cast()) },
    destroy: |owner| unsafe { xess::rsf_xessgvk_destroy(owner.cast()) },
    latency_options: Some(|owner, mode, interval| unsafe {
        xess::rsf_xessgvk_latency_options(owner.cast(), mode, interval)
    }),
    latency_sleep: Some(|owner, frame| unsafe { xess::rsf_xessgvk_sleep(owner.cast(), frame) }),
    latency_marker: Some(|owner, frame, marker| unsafe {
        xess::rsf_xessgvk_latency(owner.cast(), frame, marker)
    }),
    latency_status: Some(|owner, status| unsafe {
        xess::rsf_xessgvk_latency_status(owner.cast(), status)
    }),
};

/// The production providers. CMake's header detection selects FSR4 (`fsr4-fg-vk`, the former
/// RSF_HAVE_FSR4_FG_VK) and XeSS (`xess-fg-vk`, RSF_HAVE_XESS_FG_VK, whose bridge is C).
static NATIVE: Platform = Platform {
    dlss: Some(&DLSS_PROVIDER),
    ffx: Some(&FFX_PROVIDER),
    fsr4: if cfg!(feature = "fsr4-fg-vk") {
        Some(&FSR4_PROVIDER)
    } else {
        None
    },
    #[cfg(feature = "xess-fg-vk")]
    xess: Some(&XESS_PROVIDER),
    #[cfg(not(feature = "xess-fg-vk"))]
    xess: None,
};

// The FSR4 and XeSS providers report VkResult values and their bridge's own codes in one channel;
// the bridge codes sit outside the VkResult range because backend codes overlap it (-1, -2 and -7
// are also VkResult errors). This is the one place they become backend codes, every other failure
// a feature failure. Factory guards use this as well, while keeping C++ exceptions on their side
// of the ABI and preserving the separate initialization failure for throws.
fn rsf_core_fgvk_provider_result(backend: u32, result: i32) -> i32 {
    match (backend, result) {
        (_, OK) => OK,
        (FSR4, RSF_FSR4FG_INVALID_ARGUMENT) | (XESS, RSF_XESS_BRIDGE_INVALID_ARGUMENT) => INVALID,
        (FSR4, RSF_FSR4FG_ABI_MISMATCH) | (XESS, RSF_XESS_BRIDGE_ABI_MISMATCH) => ABI_MISMATCH,
        (FSR4, RSF_FSR4FG_NOT_SUPPORTED) => NOT_SUPPORTED,
        (FSR4 | XESS, _) => FEATURE_FAILED,
        _ => result,
    }
}

static OCCUPIED: AtomicBool = AtomicBool::new(false);
// Primary admission allows at most one retained, partially created owner.
static CLEANUP_ONLY: AtomicPtr<rsf_fgvk> = AtomicPtr::new(ptr::null_mut());

// The C ABI requires the host to serialize WSI calls and keep its callbacks
// alive through retirement. The presentation binding separately admits
// CPU latency borrows, including sleep, and refuses destruction while borrowed.
struct Owner {
    _claim: Claim,
    provider: &'static Provider,
    context: *mut c_void,
    backend: u32,
    generated_count: AtomicU32,
}

impl Owner {
    fn result(&self, result: i32) -> i32 {
        rsf_core_fgvk_provider_result(self.backend, result)
    }

    fn set_options(&self, enabled: u32, count: u32) -> i32 {
        if enabled > 1 || count == 0 {
            return INVALID;
        }
        if !supported_count(self.backend, count) {
            return NOT_SUPPORTED;
        }
        let enable = self.provider.enable;
        let result = self.result(unsafe { enable(self.context, enabled, count) });
        if result == OK {
            self.generated_count.store(count, Ordering::Relaxed);
        }
        result
    }
}

fn supported_count(backend: u32, count: u32) -> bool {
    !matches!(backend, FFX | FSR4) || count == 1
}

unsafe fn owner_ref<'a>(owner: *const rsf_fgvk) -> Option<&'a Owner> {
    unsafe { owner.cast::<Owner>().as_ref() }
}

pub(super) fn is_cleanup_only(owner: *const rsf_fgvk) -> bool {
    !owner.is_null() && CLEANUP_ONLY.load(Ordering::Acquire).cast_const() == owner
}

unsafe fn work_owner<'a>(handle: *const rsf_fgvk) -> Result<&'a Owner, i32> {
    if is_cleanup_only(handle) {
        return Err(NOT_READY);
    }
    unsafe { owner_ref(handle) }.ok_or(INVALID)
}

unsafe fn admit(
    platform: &Platform,
    setup: *const rsf_fgvk_setup,
    backend: u32,
    count: u32,
    host: *const rsf_fgvk_host_options,
) -> Result<&'static Provider, i32> {
    if !unsafe { has_capacity(setup) } {
        return Err(INVALID);
    }
    if unsafe { (*setup).abi_version } != RSF_FGVK_ABI_VERSION {
        return Err(ABI_MISMATCH);
    }
    let graphics = unsafe { addr_of!((*setup).graphics) };
    if !unsafe { has_capacity(graphics) } {
        return Err(INVALID);
    }
    if unsafe { (*graphics).abi_version } != RSF_SRVK_ABI_VERSION {
        return Err(ABI_MISMATCH);
    }
    if count == 0 {
        return Err(INVALID);
    }
    if !matches!(backend, DLSS | FFX | FSR4 | XESS) || !supported_count(backend, count) {
        return Err(NOT_SUPPORTED);
    }
    if !host.is_null() {
        if !unsafe { has_capacity(host) } {
            return Err(INVALID);
        }
        if unsafe { (*host).abi_version } != RSF_FGVK_HOST_ABI_VERSION {
            return Err(ABI_MISMATCH);
        }
    }
    let registered = match backend {
        DLSS => platform.dlss,
        FFX => platform.ffx,
        FSR4 => platform.fsr4,
        XESS => platform.xess,
        _ => unreachable!("validated provider ID"),
    };
    let provider = registered.ok_or(NOT_COMPILED)?;
    if matches!(backend, FSR4 | XESS) {
        let host = unsafe { host.as_ref() }.ok_or(INVALID)?;
        if host.window.is_null() || host.timeline_enabled != 1 {
            return Err(INVALID);
        }
        let populated_directory = if backend == FSR4 {
            let path = unsafe { (*graphics).runtime_directory_utf8 };
            !path.is_null() && unsafe { *path != 0 }
        } else {
            let path = host.intel_runtime_directory;
            !path.is_null() && unsafe { *path != 0 }
        };
        if !populated_directory {
            return Err(INVALID);
        }
    }
    Ok(provider)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_create(
    setup: *const rsf_fgvk_setup,
    out: *mut *mut rsf_fgvk,
    chain: *mut u64,
) -> i32 {
    unsafe { rsf_fgvk_create_with_host(setup, FFX, 1, ptr::null(), out, chain) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_create_with_host(
    setup: *const rsf_fgvk_setup,
    backend: u32,
    count: u32,
    host: *const rsf_fgvk_host_options,
    out: *mut *mut rsf_fgvk,
    chain: *mut u64,
) -> i32 {
    unsafe { create(&NATIVE, setup, backend, count, host, out, chain) }
}

unsafe fn create(
    platform: &Platform,
    setup: *const rsf_fgvk_setup,
    backend: u32,
    count: u32,
    host: *const rsf_fgvk_host_options,
    out: *mut *mut rsf_fgvk,
    chain: *mut u64,
) -> i32 {
    if !out.is_null() {
        unsafe { out.write(ptr::null_mut()) };
    }
    if !chain.is_null() {
        unsafe { chain.write(0) };
    }
    if out.is_null() || chain.is_null() {
        return INVALID;
    }

    let provider = match unsafe { admit(platform, setup, backend, count, host) } {
        Ok(provider) => provider,
        Err(error) => return error,
    };
    let Some(claim) = Claim::acquire(&OCCUPIED) else {
        return NOT_READY;
    };
    let creation = match ProviderOperation::create() {
        Ok(creation) => creation,
        Err(error) => return error,
    };
    let owner = allocate_handle(Owner {
        _claim: claim,
        provider,
        context: ptr::null_mut(),
        backend,
        generated_count: AtomicU32::new(1),
    });
    if owner.is_null() {
        return INIT_FAILED;
    }

    let mut owned_chain = 0;
    let create = provider.create;
    let mut result = unsafe {
        create(
            setup,
            host,
            count,
            addr_of_mut!((*owner).context),
            &mut owned_chain,
        )
    };
    let context = unsafe { (*owner).context };
    let handle = owner.cast::<rsf_fgvk>();
    if result == OK {
        result = if context.is_null() {
            INIT_FAILED
        } else {
            unsafe { (*owner).set_options(0, count) }
        };
    }
    if result == OK {
        result = creation.attach_created(handle);
    }
    if result != OK {
        if context.is_null() {
            unsafe { drop(Box::from_raw(owner)) };
            return result;
        }

        let destroy = provider.destroy;
        let cleanup = rsf_core_fgvk_provider_result(backend, unsafe { destroy(context) });
        if cleanup == OK {
            unsafe { drop(Box::from_raw(owner)) };
            return result;
        }
        // Preserve the claim and partially created provider for a destroy retry.
        CLEANUP_ONLY.store(handle, Ordering::Release);
        result = cleanup;
    }
    unsafe {
        chain.write(owned_chain);
        out.write(handle);
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_images(
    handle: *mut rsf_fgvk,
    count: *mut u32,
    images: *mut u64,
) -> i32 {
    let Ok(owner) = (unsafe { work_owner(handle) }) else {
        return WSI_INITIALIZATION_FAILED;
    };
    if count.is_null() {
        return WSI_INITIALIZATION_FAILED;
    }
    let call = owner.provider.images;
    unsafe { call(owner.context, count, images) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_acquire(
    handle: *mut rsf_fgvk,
    timeout: u64,
    semaphore: u64,
    fence: u64,
    index: *mut u32,
) -> i32 {
    let Ok(owner) = (unsafe { work_owner(handle) }) else {
        return WSI_INITIALIZATION_FAILED;
    };
    if index.is_null() {
        return WSI_INITIALIZATION_FAILED;
    }
    let call = owner.provider.acquire;
    unsafe { call(owner.context, timeout, semaphore, fence, index) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_enable(handle: *mut rsf_fgvk, enabled: u32) -> i32 {
    match unsafe { work_owner(handle) } {
        Ok(owner) => owner.set_options(enabled, owner.generated_count.load(Ordering::Relaxed)),
        Err(result) => result,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_enable_options(
    handle: *mut rsf_fgvk,
    enabled: u32,
    generated_count: u32,
) -> i32 {
    match unsafe { work_owner(handle) } {
        Ok(owner) => owner.set_options(enabled, generated_count),
        Err(result) => result,
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_fgvk_backend(
    handle: *const rsf_fgvk,
    backend: *mut u32,
) -> i32 {
    let Some(owner) = (unsafe { owner_ref(handle) }) else {
        return INVALID;
    };
    if backend.is_null() {
        return INVALID;
    }
    unsafe { backend.write(owner.backend) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_image_layout(handle: *const rsf_fgvk, layout: *mut u32) -> i32 {
    let owner = match unsafe { work_owner(handle) } {
        Ok(owner) => owner,
        Err(result) => return result,
    };
    if layout.is_null() {
        return INVALID;
    }
    match owner.provider.image_layout {
        Some(call) => unsafe { call(owner.context, layout) },
        None => {
            unsafe { layout.write(SHADER_READ) };
            OK
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_max_generated(handle: *mut rsf_fgvk, count: *mut u32) -> i32 {
    let owner = match unsafe { work_owner(handle) } {
        Ok(owner) => owner,
        Err(result) => return result,
    };
    if count.is_null() {
        return INVALID;
    }
    match owner.provider.max_generated {
        Some(call) => unsafe { call(owner.context, count) },
        None => {
            unsafe { count.write(1) };
            OK
        }
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_fgvk_latency_options(
    handle: *mut rsf_fgvk,
    mode: u32,
    minimum_interval_us: u32,
) -> i32 {
    let owner = match unsafe { work_owner(handle) } {
        Ok(owner) => owner,
        Err(result) => return result,
    };
    if mode > 1 {
        return NOT_SUPPORTED;
    }
    owner
        .provider
        .latency_options
        .map_or(NOT_SUPPORTED, |call| {
            owner.result(unsafe { call(owner.context, mode, minimum_interval_us) })
        })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_fgvk_latency_sleep(handle: *mut rsf_fgvk, frame: u64) -> i32 {
    let owner = match unsafe { work_owner(handle) } {
        Ok(owner) => owner,
        Err(result) => return result,
    };
    let Ok(frame) = u32::try_from(frame) else {
        return INVALID;
    };
    if frame == 0 {
        return INVALID;
    }
    owner.provider.latency_sleep.map_or(NOT_SUPPORTED, |call| {
        owner.result(unsafe { call(owner.context, frame) })
    })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_fgvk_latency_marker(
    handle: *mut rsf_fgvk,
    frame: u64,
    marker: u32,
) -> i32 {
    let owner = match unsafe { work_owner(handle) } {
        Ok(owner) => owner,
        Err(result) => return result,
    };
    let Ok(frame) = u32::try_from(frame) else {
        return INVALID;
    };
    if frame == 0 || marker > 6 {
        return INVALID;
    }
    owner.provider.latency_marker.map_or(NOT_SUPPORTED, |call| {
        owner.result(unsafe { call(owner.context, frame, marker) })
    })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_fgvk_latency_get_status(
    handle: *mut rsf_fgvk,
    status: *mut rsf_fgvk_latency_status,
) -> i32 {
    let owner = match unsafe { work_owner(handle) } {
        Ok(owner) => owner,
        Err(result) => return result,
    };
    if !unsafe { has_capacity(status) } {
        return INVALID;
    }
    if unsafe { (*status).abi_version } != RSF_FGVK_LATENCY_ABI_VERSION {
        return ABI_MISMATCH;
    }
    owner.provider.latency_status.map_or(NOT_SUPPORTED, |call| {
        owner.result(unsafe { call(owner.context, status) })
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_prepare(
    handle: *mut rsf_fgvk,
    commands: *mut c_void,
    frame: *const rsf_fgvk_frame,
) -> i32 {
    let owner = match unsafe { work_owner(handle) } {
        Ok(owner) => owner,
        Err(result) => return result,
    };
    let call = owner.provider.prepare;
    owner.result(unsafe { call(owner.context, commands, frame) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_hudless(
    handle: *mut rsf_fgvk,
    commands: *mut c_void,
    frame: *const rsf_fgvk_frame,
) -> i32 {
    let owner = match unsafe { work_owner(handle) } {
        Ok(owner) => owner,
        Err(result) => return result,
    };
    let call = owner.provider.hudless;
    owner.result(unsafe { call(owner.context, commands, frame) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_submitted(
    handle: *mut rsf_fgvk,
    commands: *const *mut c_void,
    count: u32,
) {
    if let Ok(owner) = unsafe { work_owner(handle) } {
        let call = owner.provider.submitted;
        unsafe { call(owner.context, commands, count) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_present(handle: *mut rsf_fgvk, info: *const c_void) -> i32 {
    let Ok(owner) = (unsafe { work_owner(handle) }) else {
        return WSI_INITIALIZATION_FAILED;
    };
    if info.is_null() {
        return WSI_INITIALIZATION_FAILED;
    }
    let call = owner.provider.present;
    unsafe { call(owner.context, info) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_present_source(
    handle: *mut rsf_fgvk,
    info: *const c_void,
    source: *const rsf_frame_record,
) -> i32 {
    let Ok(owner) = (unsafe { work_owner(handle) }) else {
        return WSI_INITIALIZATION_FAILED;
    };
    if info.is_null() || !unsafe { PresentSource::valid_prefix(source) } {
        return WSI_INITIALIZATION_FAILED;
    }
    let call = owner.provider.present_source;
    unsafe { call(owner.context, info, source) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_get_status(
    handle: *mut rsf_fgvk,
    status: *mut rsf_fgvk_status,
) -> i32 {
    let owner = match unsafe { work_owner(handle) } {
        Ok(owner) => owner,
        Err(result) => return result,
    };
    if !unsafe { has_capacity(status) } {
        return INVALID;
    }
    if unsafe { (*status).abi_version } != RSF_FGVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let call = owner.provider.status;
    owner.result(unsafe { call(owner.context, status) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_drain(handle: *mut rsf_fgvk) -> i32 {
    let Some(owner) = (unsafe { owner_ref(handle) }) else {
        return OK;
    };
    let _operation = match ProviderOperation::drain(handle) {
        Ok(operation) => operation,
        Err(error) => return error,
    };
    let drain = owner.provider.drain;
    owner.result(unsafe { drain(owner.context) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_destroy(handle: *mut rsf_fgvk) -> i32 {
    let Some(owner) = (unsafe { owner_ref(handle) }) else {
        return OK;
    };
    let mut operation = match ProviderOperation::destroy(handle) {
        Ok(operation) => operation,
        Err(error) => return error,
    };
    let destroy = owner.provider.destroy;
    let result = owner.result(unsafe { destroy(owner.context) });
    if result == OK {
        operation.destroyed();
        if is_cleanup_only(handle) {
            CLEANUP_ONLY.store(ptr::null_mut(), Ordering::Release);
        }
        unsafe { drop(Box::from_raw(handle.cast::<Owner>())) };
    }
    result
}

#[cfg(test)]
#[path = "vulkan_fg/dispatch_tests.rs"]
mod dispatch_tests;
