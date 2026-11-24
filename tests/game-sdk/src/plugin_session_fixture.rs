// SPDX-License-Identifier: MIT
use rsf_game_sys::{RSF_ERROR_ABI_MISMATCH, RSF_ERROR_BUSY, RSF_ERROR_NOT_READY, RSF_OK};
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_detect_game_fn, rsf_detection, rsf_game_control_args,
    rsf_game_hooks_api, rsf_game_host_services, rsf_game_plugin_api, rsf_game_prepare_args,
    rsf_game_probe, rsf_game_renderer_status, rsf_game_start_args, rsf_result,
};
use std::ffi::CStr;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

static HOST: AtomicPtr<rsf_game_host_services> = AtomicPtr::new(ptr::null_mut());
static QUIESCE_CALLS: AtomicU32 = AtomicU32::new(0);
static STOP_CALLS: AtomicU32 = AtomicU32::new(0);
const UNKNOWN: rsf_detection = 0;
const RECOGNIZED: rsf_detection = 1;

// The callback records share the size and version prefix.
unsafe fn valid_args<T>(args: *const T) -> bool {
    let header = args.cast::<u32>();
    !header.is_null()
        && unsafe { header.read() } as usize == size_of::<T>()
        && unsafe { header.add(1).read() } == RSF_GAME_ABI_VERSION
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_session_detect(
    probe: *const rsf_game_probe,
    result: *mut rsf_detection,
) -> bool {
    let machine = unsafe { (*probe).pe_machine };
    if machine == 2 {
        return false;
    }
    unsafe { result.write(if machine == 1 { UNKNOWN } else { RECOGNIZED }) };
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_session_prepare(
    args: *const rsf_game_prepare_args,
    result: *mut rsf_result,
) -> bool {
    if !unsafe { valid_args(args) } || unsafe { (*args).host.is_null() } {
        unsafe { result.write(RSF_ERROR_ABI_MISMATCH) };
        return true;
    }
    let host = unsafe { (*args).host };
    // Publish the host before simulating a failure so rollback can borrow it.
    HOST.store(host.cast_mut(), Ordering::Release);
    if unsafe { (*host).session_id } == 2 {
        return false;
    }
    unsafe { result.write(RSF_OK) };
    true
}

fn log_operation(operation: &CStr) -> rsf_result {
    // The fixture supplies a Rust log callback and retains its host through retirement.
    let Some(host) = (unsafe { HOST.load(Ordering::Acquire).as_ref() }) else {
        return RSF_ERROR_NOT_READY;
    };
    if let Some(log) = host.log {
        unsafe { log(host.user, operation.as_ptr()) };
    }
    RSF_OK
}

unsafe extern "C" fn start(args: *const rsf_game_start_args) -> rsf_result {
    if !unsafe { valid_args(args) } {
        return RSF_ERROR_ABI_MISMATCH;
    }
    log_operation(c"start")
}

unsafe extern "C" fn quiesce(args: *const rsf_game_control_args) -> rsf_result {
    if !unsafe { valid_args(args) } {
        return RSF_ERROR_ABI_MISMATCH;
    }
    let Some(host) = (unsafe { HOST.load(Ordering::Acquire).as_ref() }) else {
        return RSF_ERROR_NOT_READY;
    };
    let previous = QUIESCE_CALLS.fetch_add(1, Ordering::Relaxed);
    if matches!(host.session_id, 2 | 3) && previous == 0 {
        RSF_ERROR_BUSY
    } else {
        RSF_OK
    }
}

unsafe extern "C" fn stop(args: *const rsf_game_control_args) -> rsf_result {
    if !unsafe { valid_args(args) } {
        return RSF_ERROR_ABI_MISMATCH;
    }
    let Some(host) = (unsafe { HOST.load(Ordering::Acquire).as_ref() }) else {
        return RSF_ERROR_NOT_READY;
    };
    let previous = STOP_CALLS.fetch_add(1, Ordering::Relaxed);
    if host.session_id == 2 && previous == 0 {
        RSF_ERROR_BUSY
    } else {
        RSF_OK
    }
}

extern "C" fn status(_: *mut rsf_game_renderer_status) -> rsf_result {
    log_operation(c"status")
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_session_query(
    abi: u32,
    api: *mut rsf_game_plugin_api,
    detect: rsf_detect_game_fn,
    prepare: Option<unsafe extern "C" fn(*const rsf_game_prepare_args) -> rsf_result>,
) -> rsf_result {
    if abi != RSF_GAME_ABI_VERSION
        || api.is_null()
        || (unsafe { api.cast::<u32>().read() } as usize) < size_of::<rsf_game_plugin_api>()
    {
        return RSF_ERROR_ABI_MISMATCH;
    }
    unsafe {
        (*api).abi_version = abi;
        (*api).detect = detect;
        (*api).hooks = rsf_game_hooks_api {
            prepare,
            start: Some(start),
            quiesce: Some(quiesce),
            stop: Some(stop),
            status: Some(status),
        };
    }
    RSF_OK
}
