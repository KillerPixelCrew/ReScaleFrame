// SPDX-License-Identifier: MIT
//! Godot 4.7.2 native integration owner linked into `ReScaleFrame.Godot.Native.dll`. It owns the
//! Runtime control, settings and D3D12 session state (`control.rs`), the D3D12 SR context and
//! submission owner (`d3d12.rs`), the Vulkan session state (`vulkan.rs`) and the
//! `ReScaleFrameRuntime` option schema and renderer admission (`extension.rs`).
//!
//! The `rsfg_*` exports in `api.h`, the GDExtension entry and the godot-cpp classes stay C++.
//! `runtime.cpp` and `vulkan.cpp` forward to these entry points and lend their leaves through
//! `rsf_godot_platform` (`godot_platform.h`, mirrored in `platform.rs`): the D3D12 device, fence,
//! SR context, FG, presentation and capture calls, DXGI, Win32 window calls, the linked Runtime
//! and the Vulkan owner's guard, device, provider, presenter and SDK calls. No second Runtime is
//! loaded and no vendor, pacing or presentation policy is added here.
mod control;
mod d3d12;
mod extension;
mod platform;
mod vulkan;

use control::Control;
use platform::{
    ABI_VERSION, EngineServices, OptionField, OptionValue, Platform, SETTINGS_ABI_VERSION,
    Settings, Setup, Status, VkFgPass, VkPresentFacts, VkSrFrame, VkView,
};
use rsf_game_sys::rsf_game_render_pass;
use std::ffi::{CStr, c_char, c_int, c_void};
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
    unsafe { leaves(table) }.map_or(0, |_| CONTROL.configure_engine(settings, viewport_id))
}

/// # Safety
/// `table` is null or the leaf table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_reset_history(table: *const Platform) {
    // SAFETY: the caller's guarantee.
    if unsafe { leaves(table) }.is_some() {
        CONTROL.reset_history();
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
        (Some(_), Some(settings)) => unsafe { CONTROL.settings(settings) },
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

// The D3D12 SR context and submission owner (d3d12.rs), called by the rsfg_* forwarders in
// runtime.cpp on Godot's render thread.

/// # Safety
/// `table` is null or the leaf table; `pass` is null or a readable `rsf_game_render_pass` of its
/// stated size; `device`, `queue` and `commands` are the engine's live D3D12 objects for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_d3d12_render(
    table: *const Platform,
    device: *mut c_void,
    queue: *mut c_void,
    commands: *mut c_void,
    pass: *const rsf_game_render_pass,
) -> c_int {
    // SAFETY: the caller's guarantee.
    let (Some(platform), Some(pass)) = (unsafe { leaves(table) }, unsafe { pass.as_ref() }) else {
        return 0;
    };
    if !CONTROL.booted()
        || device.is_null()
        || queue.is_null()
        || commands.is_null()
        || (pass.struct_size as usize) < size_of::<rsf_game_render_pass>()
    {
        return 0;
    }
    CONTROL
        .d3d12
        .render(&CONTROL, platform, device, queue, commands, pass)
}

/// # Safety
/// `table` is null or the leaf table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_d3d12_render_begin(table: *const Platform, frame: u64) {
    // SAFETY: the caller's guarantee.
    if let Some(platform) = unsafe { leaves(table) }
        && CONTROL.booted()
    {
        CONTROL.d3d12.render_begin(&CONTROL, platform, frame);
    }
}

/// # Safety
/// `table` is null or the leaf table; `lists` is null or readable for `count` entries.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_d3d12_submitted(
    table: *const Platform,
    queue: *mut c_void,
    lists: *const *mut c_void,
    count: u32,
) {
    // SAFETY: the caller's guarantee.
    let Some(platform) = (unsafe { leaves(table) }) else {
        return;
    };
    if queue.is_null() || lists.is_null() || count == 0 {
        return;
    }
    // SAFETY: `count` readable command list pointers, compared by address only.
    let lists = unsafe { std::slice::from_raw_parts(lists.cast::<usize>(), count as usize) };
    CONTROL.d3d12.submitted(platform, queue, lists);
}

/// # Safety
/// `table` is null or the leaf table; `chain` is the engine's swapchain for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_d3d12_present(table: *const Platform, chain: *mut c_void) {
    // SAFETY: the caller's guarantee.
    if let Some(platform) = unsafe { leaves(table) }
        && CONTROL.booted()
    {
        CONTROL.d3d12.present(&CONTROL, platform, chain);
    }
}

/// # Safety
/// `table` is null or the leaf table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_d3d12_shutdown(table: *const Platform) -> c_int {
    // SAFETY: the caller's guarantee.
    unsafe { leaves(table) }.map_or(0, |platform| CONTROL.d3d12.shutdown(&CONTROL, platform))
}

// The Vulkan session (vulkan.rs). The exports below other than boot and shutdown are called by
// the leaves in vulkan.cpp while they hold its guard.

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_booted() -> c_int {
    CONTROL.vulkan.booted().into()
}

/// # Safety
/// `table` is null or the leaf table; `setup` is null or a readable `rsf_godot_setup` of its
/// stated size whose strings are null or NUL-terminated for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_boot_vulkan(table: *const Platform, setup: *const Setup) -> c_int {
    // SAFETY: the caller's guarantee.
    match unsafe { (leaves(table), checked(setup, ABI_VERSION)) } {
        (Some(platform), Some(setup)) => CONTROL.vulkan.boot(platform, setup),
        _ => 0,
    }
}

/// # Safety
/// `table` is null or the leaf table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_shutdown_vulkan(table: *const Platform) -> c_int {
    // SAFETY: the caller's guarantee.
    unsafe { leaves(table) }.map_or(0, |platform| CONTROL.vulkan.shutdown(platform))
}

/// # Safety
/// `view` is null or writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_view_get(view: *mut VkView) -> c_int {
    let (value, active) = CONTROL.vulkan.view();
    if !view.is_null() {
        // SAFETY: the caller's guarantee.
        unsafe { view.write(value) };
    }
    active
}

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_presenter_created(ready: u32, backend: u32, max_generated: u32) {
    CONTROL
        .vulkan
        .presenter_created(ready != 0, backend, max_generated);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_presenter_current(backend: u32) -> c_int {
    CONTROL.vulkan.presenter_current(backend).into()
}

/// # Safety
/// `facts` is null or readable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_presented(facts: *const VkPresentFacts) {
    // SAFETY: the caller's guarantee.
    if let Some(facts) = unsafe { facts.as_ref() } {
        CONTROL.vulkan.presented(facts);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_presenter_retired() {
    CONTROL.vulkan.presenter_retired();
}

/// # Safety
/// `pass` is null or readable and writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_fg_begin(pass: *mut VkFgPass) {
    // SAFETY: the caller's guarantee.
    if let Some(pass) = unsafe { pass.as_mut() } {
        CONTROL.vulkan.fg_begin(pass);
    }
}

/// # Safety
/// `pass` is null or readable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_fg_admit(pass: *const VkFgPass) -> c_int {
    // SAFETY: the caller's guarantee.
    unsafe { pass.as_ref() }
        .is_some_and(|pass| CONTROL.vulkan.fg_admit(pass))
        .into()
}

/// # Safety
/// `pass` is null or readable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_fg_recorded(pass: *const VkFgPass, result: i32) {
    // SAFETY: the caller's guarantee.
    if let Some(pass) = unsafe { pass.as_ref() } {
        CONTROL.vulkan.fg_recorded(pass, result);
    }
}

/// # Safety
/// `frame` is null or readable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_sr_frame_report(frame: *const VkSrFrame) {
    // SAFETY: the caller's guarantee.
    if let Some(frame) = unsafe { frame.as_ref() } {
        CONTROL.vulkan.sr_frame(frame);
    }
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
