// SPDX-License-Identifier: MIT
//! Godot 4.7.2 native integration owner linked into `ReScaleFrame.Godot.Native.dll`. This first
//! slice owns the Runtime control, settings and session state (`control.rs`) and the
//! `ReScaleFrameRuntime` option schema and renderer admission (`extension.rs`).
//!
//! The `rsfg_*` exports in `api.h`, the GDExtension entry and the godot-cpp classes stay C++.
//! `runtime.cpp` forwards to these entry points and lends its leaves through
//! `rsf_godot_platform` (`godot_platform.h`, mirrored in `platform.rs`): the D3D12 SR context,
//! fences and capture, DXGI, Win32 window calls, the linked Runtime and the Vulkan owner. No
//! second Runtime is loaded and no vendor, pacing or presentation policy is added here.
mod control;
mod extension;
mod platform;

use control::Control;
use platform::{
    ABI_VERSION, EngineServices, OptionField, OptionValue, Paths, Platform, SETTINGS_ABI_VERSION,
    Settings, Setup, SrFrame, Status,
};
use std::ffi::{CStr, c_char, c_int};
use std::sync::LazyLock;

static CONTROL: LazyLock<Control> = LazyLock::new(Control::new);

/// A size- and ABI-checked record: `struct_size` and `abi_version` lead every one of them.
///
/// # Safety
/// `record` is null or readable for its leading eight bytes, and for `T` when they pass.
unsafe fn checked<'a, T>(record: *const T, abi: u32) -> Option<&'a T> {
    // SAFETY: the caller's guarantee for the header.
    let [size, version] = unsafe { record.cast::<[u32; 2]>().as_ref() }.copied()?;
    // SAFETY: the header covers T.
    (size as usize >= size_of::<T>() && version == abi).then(|| unsafe { &*record })
}

/// As `checked`, for a caller-owned output record.
///
/// # Safety
/// As `checked`, and writable for `T` when the header passes.
unsafe fn checked_output<T>(record: *mut T, abi: u32) -> Option<*mut T> {
    // SAFETY: the caller's guarantee.
    unsafe { checked(record.cast_const(), abi) }.map(|_| record)
}

/// # Safety
/// `table` is null or the process-lived leaf table.
unsafe fn leaves(table: *const Platform) -> Option<&'static Platform> {
    // SAFETY: the caller's guarantee.
    unsafe { platform::register(table) }
}

/// # Safety
/// `services` is null or a readable `rsf_godot_engine_services` of its stated size.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_attach_engine(services: *const EngineServices) -> c_int {
    // SAFETY: the caller's guarantee.
    unsafe { checked(services, ABI_VERSION) }
        .is_some_and(|services| CONTROL.attach_engine(services))
        .into()
}

/// # Safety
/// `table` is null or the leaf table; `settings` is forwarded to the engine module unchanged.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_configure_engine(
    table: *const Platform,
    settings: *const Settings,
    viewport_id: u64,
) -> c_int {
    // SAFETY: the caller's guarantee.
    unsafe { leaves(table) }.map_or(0, |platform| {
        CONTROL.configure_engine(platform, settings, viewport_id)
    })
}

/// # Safety
/// `table` is null or the leaf table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_reset_history(table: *const Platform) {
    // SAFETY: the caller's guarantee.
    if let Some(platform) = unsafe { leaves(table) } {
        CONTROL.reset_history(platform);
    }
}

/// # Safety
/// `table` is null or the leaf table; `setup` is null or a readable `rsf_godot_setup` of its
/// stated size whose strings are null or NUL-terminated for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_boot(table: *const Platform, setup: *const Setup) -> c_int {
    // SAFETY: the caller's guarantee.
    match unsafe { (leaves(table), checked(setup, ABI_VERSION)) } {
        (Some(platform), Some(setup)) => CONTROL.boot(platform, setup),
        _ => 0,
    }
}

/// # Safety
/// `table` is null or the leaf table; `settings` is null or a readable `rsf_godot_settings` of
/// its stated size.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_configure(
    table: *const Platform,
    settings: *const Settings,
) -> c_int {
    // SAFETY: the caller's guarantee.
    match unsafe { (leaves(table), checked(settings, SETTINGS_ABI_VERSION)) } {
        (Some(platform), Some(settings)) => CONTROL.configure(platform, settings),
        _ => 0,
    }
}

/// # Safety
/// `table` is null or the leaf table; `settings` is null or a writable `rsf_godot_settings` of
/// its stated size.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_settings_get(
    table: *const Platform,
    settings: *mut Settings,
) -> c_int {
    // SAFETY: the caller's guarantee.
    match unsafe {
        (
            leaves(table),
            checked_output(settings, SETTINGS_ABI_VERSION),
        )
    } {
        // SAFETY: checked_output established the record.
        (Some(platform), Some(settings)) => unsafe { CONTROL.settings(platform, settings) },
        _ => 0,
    }
}

/// # Safety
/// `table` is null or the leaf table; `status` is null or a writable `rsf_godot_status` of its
/// stated size.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_status_get(table: *const Platform, status: *mut Status) -> c_int {
    // SAFETY: the caller's guarantee.
    match unsafe { (leaves(table), checked_output(status, ABI_VERSION)) } {
        // SAFETY: checked_output established the record.
        (Some(platform), Some(status)) => unsafe { CONTROL.status(platform, status) },
        _ => 0,
    }
}

/// `ReScaleFrameRuntime.get_status` reports the engine bridge ready only for a live session.
///
/// # Safety
/// As `rsf_godot_status_get`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_status_ready(table: *const Platform, status: *mut Status) -> c_int {
    // SAFETY: the caller's guarantee; a successful read filled the record.
    let ready =
        unsafe { rsf_godot_status_get(table, status) } != 0 && unsafe { (*status).session_id } != 0;
    ready.into()
}

/// `message` is forwarded to the session log unchanged, as the C++ trampoline did.
#[unsafe(no_mangle)]
extern "C" fn rsf_godot_log(message: *const c_char) {
    CONTROL.log(message);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_booted() -> c_int {
    CONTROL.booted().into()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_session_id() -> u64 {
    CONTROL.session_id()
}

/// # Safety
/// `paths` is null or writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_paths_get(paths: *mut Paths) -> c_int {
    match (CONTROL.paths(), paths.is_null()) {
        (Some(value), false) => {
            // SAFETY: the caller's guarantee.
            unsafe { paths.write(value) };
            1
        }
        _ => 0,
    }
}

/// # Safety
/// Each output is null or writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_settings_snapshot(settings: *mut Settings, generation: *mut u64) {
    let (value, current) = CONTROL.snapshot();
    // SAFETY: the caller's guarantee.
    unsafe {
        if !settings.is_null() {
            settings.write(value);
        }
        if !generation.is_null() {
            generation.write(current);
        }
    }
}

/// # Safety
/// `frame` is null or readable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_sr_frame_report(frame: *const SrFrame) {
    // SAFETY: the caller's guarantee.
    if let Some(frame) = unsafe { frame.as_ref() } {
        CONTROL.sr_frame(frame);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_sr_missed() {
    CONTROL.sr_missed();
}

/// # Safety
/// `count` is null or writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_option_fields(count: *mut u32) -> *const OptionField {
    if !count.is_null() {
        // SAFETY: the caller's guarantee.
        unsafe { count.write(extension::FIELD_COUNT as u32) };
    }
    extension::FIELDS.0.as_ptr()
}

/// # Safety
/// `table` is null or the leaf table; `values` is null or readable for `count` values;
/// `settings` is null or writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_compose_settings(
    table: *const Platform,
    values: *const OptionValue,
    count: u32,
    settings: *mut Settings,
) -> c_int {
    if count as usize != extension::FIELD_COUNT || settings.is_null() {
        return 0;
    }
    // SAFETY: the caller's guarantee for the table and FIELD_COUNT values.
    let (Some(platform), Some(values)) = (unsafe { leaves(table) }, unsafe {
        values
            .cast::<[OptionValue; extension::FIELD_COUNT]>()
            .as_ref()
    }) else {
        return 0;
    };
    match extension::compose(platform, values) {
        Some(value) => {
            // SAFETY: the caller's guarantee.
            unsafe { settings.write(value) };
            1
        }
        None => 0,
    }
}

/// # Safety
/// `driver` and `method` are null or NUL-terminated for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_renderer_admission(
    driver: *const c_char,
    method: *const c_char,
    vulkan_supported: u32,
) -> u32 {
    let name = |value: *const c_char| {
        // SAFETY: the caller's guarantee.
        (!value.is_null()).then(|| unsafe { CStr::from_ptr(value) }.to_bytes())
    };
    extension::admission(name(driver), name(method), vulkan_supported != 0)
}
