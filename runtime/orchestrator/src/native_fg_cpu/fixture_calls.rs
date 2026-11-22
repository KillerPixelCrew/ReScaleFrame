// SPDX-License-Identifier: MIT
//! Private C fixture entry points. Each call may supply different leaves.

use super::*;
use std::ffi::c_char;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_fg_cpu(event: *const Event, raw: *const rsf_nfg_cpu_platform) {
    let Some(p) = (unsafe { raw.as_ref() }).and_then(Platform::checked) else {
        return;
    };
    unsafe { fg_cpu(event, &p) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_fg_frame(
    pass: *const Pass,
    begin: u32,
    raw: *const rsf_nfg_cpu_platform,
) {
    let Some(p) = (unsafe { raw.as_ref() }).and_then(Platform::checked) else {
        return;
    };
    unsafe { fg_frame(pass, begin, &p) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_fg_submission(
    pass: *const Pass,
    begin: u32,
    raw: *const rsf_nfg_cpu_platform,
) {
    let Some(p) = (unsafe { raw.as_ref() }).and_then(Platform::checked) else {
        return;
    };
    unsafe { fg_submission(pass, begin, &p) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_latency_render(
    id: u64,
    begin: u32,
    raw: *const rsf_nfg_cpu_platform,
) {
    let Some(p) = (unsafe { raw.as_ref() }).and_then(Platform::checked) else {
        return;
    };
    latency_render(id, begin, &p)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_latency_present(
    id: u64,
    chain: Handle,
    begin: u32,
    raw: *const rsf_nfg_cpu_platform,
) {
    let Some(p) = (unsafe { raw.as_ref() }).and_then(Platform::checked) else {
        return;
    };
    observe_latency_present(id, chain, begin, &p)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_latency_window_bind(
    binding: *const Window,
    raw: *const rsf_nfg_cpu_platform,
) -> i32 {
    let Some(p) = (unsafe { raw.as_ref() }).and_then(Platform::checked) else {
        return INVALID;
    };
    unsafe { latency_window_bind(binding, &p) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_latency_status_get(
    out: *mut LatencyStatus,
    raw: *const rsf_nfg_cpu_platform,
) -> i32 {
    let Some(p) = (unsafe { raw.as_ref() }).and_then(Platform::checked) else {
        return 0;
    };
    unsafe { latency_status_get(out, &p) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_latency_ping(
    window: Handle,
    raw: *const rsf_nfg_cpu_platform,
) -> i32 {
    let Some(p) = (unsafe { raw.as_ref() }).and_then(Platform::checked) else {
        return 0;
    };
    latency_ping(window, &p)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_latency_retire(
    session: u64,
    through: u64,
    raw: *const rsf_nfg_cpu_platform,
) {
    let Some(p) = (unsafe { raw.as_ref() }).and_then(Platform::checked) else {
        return;
    };
    latency_retire(session, through, &p)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_latency_end_session(
    session: u64,
    raw: *const rsf_nfg_cpu_platform,
) {
    let Some(p) = (unsafe { raw.as_ref() }).and_then(Platform::checked) else {
        return;
    };
    latency_end_session(session, &p)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_fg_stage(
    name: *const c_char,
    source: u64,
    native: u64,
    result: i32,
    raw: *const rsf_nfg_cpu_platform,
) {
    let Some(p) = (unsafe { raw.as_ref() }).and_then(Platform::checked) else {
        return;
    };
    unsafe { diagnostics::report_stage(name, source, native, result, &p) }
}
