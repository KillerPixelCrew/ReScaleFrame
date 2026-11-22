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

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_COMPILED: i32 = -3;
const INIT_FAILED: i32 = -6;
const NOT_SUPPORTED: i32 = -7;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;
const WSI_INITIALIZATION_FAILED: i32 = -3;

const DLSS: u32 = 1;
const FFX: u32 = 3;
const FSR4: u32 = 4;
const XESS: u32 = 5;

// Factory guards use this as well, while keeping C++ exceptions on their side
// of the ABI and preserving the separate initialization failure for throws.
#[unsafe(no_mangle)]
extern "C" fn rsf_core_fgvk_provider_result(backend: u32, result: i32) -> i32 {
    match (backend, result) {
        (_, OK) => OK,
        (FSR4, -13001) | (XESS, -10001) => INVALID,
        (FSR4, -13002) | (XESS, -10002) => ABI_MISMATCH,
        (FSR4, -13003) => NOT_SUPPORTED,
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
    provider: &'static rsf_fgvk_provider,
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
        let enable = self.provider.enable.expect("provider enable callback");
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
    platform: &rsf_fgvk_platform,
    setup: *const rsf_fgvk_setup,
    backend: u32,
    count: u32,
    host: *const rsf_fgvk_host_options,
) -> Result<&rsf_fgvk_provider, i32> {
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
    let provider = unsafe { registered.as_ref() }.ok_or(NOT_COMPILED)?;
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
unsafe extern "C" fn rsf_core_fgvk_create(
    platform: *const rsf_fgvk_platform,
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

    // The native shim supplies immutable tables that live for the module lifetime.
    let platform = unsafe { &*platform };
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
    let create = provider.create.expect("provider create callback");
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

        let destroy = provider.destroy.expect("provider destroy callback");
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
    let call = owner.provider.images.expect("provider images callback");
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
    let call = owner.provider.acquire.expect("provider acquire callback");
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
unsafe extern "C" fn rsf_fgvk_backend(handle: *const rsf_fgvk, backend: *mut u32) -> i32 {
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
unsafe extern "C" fn rsf_fgvk_latency_options(
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
unsafe extern "C" fn rsf_fgvk_latency_sleep(handle: *mut rsf_fgvk, frame: u64) -> i32 {
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
unsafe extern "C" fn rsf_fgvk_latency_marker(
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
unsafe extern "C" fn rsf_fgvk_latency_get_status(
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
    let call = owner.provider.prepare.expect("provider prepare callback");
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
    let call = owner.provider.hudless.expect("provider HUD-less callback");
    owner.result(unsafe { call(owner.context, commands, frame) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fgvk_submitted(
    handle: *mut rsf_fgvk,
    commands: *const *mut c_void,
    count: u32,
) {
    if let Ok(owner) = unsafe { work_owner(handle) } {
        let call = owner
            .provider
            .submitted
            .expect("provider submitted callback");
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
    let call = owner.provider.present.expect("provider present callback");
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
    let call = owner
        .provider
        .present_source
        .expect("provider source present callback");
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
    let call = owner.provider.status.expect("provider status callback");
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
    let drain = owner.provider.drain.expect("provider drain callback");
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
    let destroy = owner.provider.destroy.expect("provider destroy callback");
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
