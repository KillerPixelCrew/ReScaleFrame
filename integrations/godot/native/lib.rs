// SPDX-License-Identifier: MIT
//! Godot 4.7.2 native integration owner linked into `ReScaleFrame.Godot.Native.dll`. It owns the
//! Runtime control, settings and D3D12 session state (`control.rs`), the D3D12 SR context and
//! submission owner (`d3d12.rs`), the Vulkan session state (`vulkan.rs`) with its SR context
//! owner (`vulkan/sr.rs`) and device creation owner (`vulkan/creation.rs`), the input window, its procedure and the input session (`window.rs`),
//! the `ReScaleFrameRuntime` option schema and renderer admission (`extension.rs`) and each
//! `ReScaleFrameEffect`'s probe snapshot and motion capture classification (`effect.rs`).
//!
//! The `rsfg_*` exports in `api.h`, the GDExtension entry and the godot-cpp classes stay C++.
//! `runtime.cpp` and `vulkan.cpp` forward to these entry points and lend their leaves through
//! `rsf_godot_platform` (`godot_platform.h`, mirrored in `platform.rs`): the D3D12 device, fence,
//! SR context, FG, presentation and capture calls, DXGI, Win32 window calls, the linked Runtime
//! and the Vulkan owner's guard, device, provider, presenter and SDK calls. `vulkan_creation.cpp`
//! lends its Vulkan-typed creation leaves with each creation call (`rsf_godot_vk_creation_leaves`),
//! and `effect.cpp` its godot-cpp leaves with each compositor callback (`rsf_godot_effect_leaves`).
//! No second Runtime is
//! loaded and no vendor, pacing or presentation policy is added here.
mod control;
mod d3d12;
mod effect;
mod extension;
mod platform;
mod vulkan;
mod window;

use control::Control;
use effect::Effect;
use platform::{
    ABI_VERSION, EFFECT_VERSION, EffectLeaves, EffectStatus, EngineServices, OptionField,
    OptionValue, Platform, SETTINGS_ABI_VERSION, Settings, Setup, Status, VkCreationLeaves,
    VkDeviceCall, VkFgPass, VkFunction, VkInstanceCall, VkPresentFacts, VkProc, VkSrPass, VkView,
};
use rsf_game_sys::{rsf_game_cpu_event, rsf_game_log_fn, rsf_game_render_pass};
use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::LazyLock;

static CONTROL: LazyLock<Control> = LazyLock::new(Control::new);

/// `VK_ERROR_INITIALIZATION_FAILED`, for a creation call without its table or record.
const VK_ERROR_INITIALIZATION_FAILED: i32 = -3;

/// # Safety
/// A non-null record has a readable size word. When the size covers T, its version word is
/// readable too. T is a C record with two leading u32 header fields.
unsafe fn header_valid<T>(record: *const T, abi: u32) -> bool {
    if record.is_null() {
        return false;
    }
    let header = record.cast::<u32>();
    // SAFETY: the caller guarantees the size word, and a sufficient size covers the version.
    unsafe { header.read() as usize >= size_of::<T>() && header.add(1).read() == abi }
}

/// # Safety
/// As for header_valid. A matching header covers a fully initialized T.
unsafe fn checked<'a, T>(record: *const T, abi: u32) -> Option<&'a T> {
    // SAFETY: the caller guarantees a readable initialized record when the header matches.
    unsafe { header_valid(record, abi) }.then(|| unsafe { &*record })
}

/// # Safety
/// As for header_valid. A matching header covers writable storage for T.
unsafe fn checked_output<T>(record: *mut T, abi: u32) -> Option<*mut T> {
    // SAFETY: the header is initialized independently of the rest of the record.
    unsafe { header_valid(record.cast_const(), abi) }.then_some(record)
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
unsafe extern "C" fn rsfg_attach_engine(services: *const EngineServices) -> c_int {
    // SAFETY: the caller's guarantee.
    unsafe { checked(services, ABI_VERSION) }
        .is_some_and(|services| CONTROL.attach_engine(services))
        .into()
}

/// # Safety
/// `settings` follows the checked record contract.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_configure_engine(settings: *const Settings, viewport_id: u64) -> c_int {
    // SAFETY: the caller's guarantee.
    unsafe { checked(settings, SETTINGS_ABI_VERSION) }.map_or(0, |settings| {
        CONTROL.configure_engine(settings, viewport_id)
    })
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_reset_history() {
    CONTROL.reset_history();
}

/// # Safety
/// `table` is null or the leaf table; `setup` is null or a readable `rsf_godot_setup` of its
/// stated size whose strings are null or NUL-terminated for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_boot(table: *const Platform, setup: *const Setup) -> c_int {
    // SAFETY: the caller's guarantee.
    match unsafe { (leaves(table), checked(setup, ABI_VERSION)) } {
        (Some(platform), Some(setup)) => {
            let result = CONTROL.boot(platform, setup);
            if result != 0 {
                CONTROL.window.bind_cpu_service(platform.cpu_service(false));
            }
            result
        }
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
/// `settings` follows the checked output record contract.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_settings(settings: *mut Settings) -> c_int {
    // SAFETY: the caller's guarantee; a matching header covers writable storage.
    unsafe { checked_output(settings, SETTINGS_ABI_VERSION) }
        .map_or(0, |settings| unsafe { CONTROL.settings(settings) })
}

/// # Safety
/// `status` follows the checked output record contract.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_status(status: *mut Status) -> c_int {
    // SAFETY: the caller's guarantee.
    match (platform::registered(), unsafe {
        checked_output(status, ABI_VERSION)
    }) {
        // SAFETY: checked_output established the record.
        (Some(platform), Some(status)) => unsafe { CONTROL.status(platform, status) },
        _ => 0,
    }
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
/// `lists` is null or readable for `count` entries.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_submitted(queue: *mut c_void, lists: *const *mut c_void, count: u32) {
    let Some(platform) = (platform::registered()) else {
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
/// `chain` is the engine's swapchain for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_present(chain: *mut c_void) {
    if let Some(platform) = platform::registered()
        && CONTROL.booted()
        && !chain.is_null()
    {
        CONTROL.d3d12.present(&CONTROL, platform, chain);
    }
}

/// # Safety
/// `table` is null or the leaf table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_d3d12_shutdown(table: *const Platform) -> c_int {
    // SAFETY: the caller's guarantee.
    let result =
        unsafe { leaves(table) }.map_or(0, |platform| CONTROL.d3d12.shutdown(&CONTROL, platform));
    if result != 0 {
        CONTROL.window.bind_cpu_service(None);
    }
    result
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
        (Some(platform), Some(setup)) => {
            let result = CONTROL.vulkan.boot(platform, setup);
            if result != 0 {
                CONTROL.window.bind_cpu_service(platform.cpu_service(true));
            }
            result
        }
        _ => 0,
    }
}

/// # Safety
/// `table` is null or the leaf table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_shutdown_vulkan(table: *const Platform) -> c_int {
    // SAFETY: the caller's guarantee.
    let result = unsafe { leaves(table) }.map_or(0, |platform| CONTROL.vulkan.shutdown(platform));
    if result != 0 {
        CONTROL.window.bind_cpu_service(None);
    }
    result
}

/// The view with Auto resolved through the table the boot registered. Without one (no boot was
/// ever attempted) nothing is written and zero is returned.
///
/// # Safety
/// `view` is null or writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_view_get(view: *mut VkView) -> c_int {
    let Some(platform) = platform::registered() else {
        return 0;
    };
    let (value, active) = CONTROL.vulkan.view(platform);
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

/// Zero, so the engine recreates its WSI owner, also without a registered table (not reachable:
/// a presenter exists only after a boot registered one).
#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_presenter_current(backend: u32) -> c_int {
    platform::registered()
        .is_some_and(|platform| CONTROL.vulkan.presenter_current(platform, backend))
        .into()
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

/// The SR section of `rsfg_render_vulkan`, called with the guard held.
///
/// # Safety
/// `table` is null or the leaf table; `pass` is null or a readable `rsf_godot_vk_sr_pass` whose
/// `call` record stays live for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_sr_render(
    table: *const Platform,
    pass: *const VkSrPass,
) -> c_int {
    // SAFETY: the caller's guarantee.
    match unsafe { (leaves(table), pass.as_ref()) } {
        (Some(platform), Some(pass)) => CONTROL.vulkan.sr_render(platform, pass),
        _ => 0,
    }
}

/// # Safety
/// `context` and `fsr4_context` are null or writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_sr_contexts(
    context: *mut *mut c_void,
    fsr4_context: *mut *mut c_void,
) {
    let (sr, fsr4) = CONTROL.vulkan.sr_contexts();
    // SAFETY: the caller's guarantee.
    unsafe {
        if !context.is_null() {
            context.write(sr);
        }
        if !fsr4_context.is_null() {
            fsr4_context.write(fsr4);
        }
    }
}

// The Vulkan creation owner (vulkan/creation.rs), reached from vulkan_creation.cpp without the
// guard: the start and stop leaves and the resolver's entry points on the engine's thread.

/// The start leaf's underlying resolver, XeSS preparation and session log. Nonzero when the
/// resolver is installed.
#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_creation_begin(
    underlying: Option<VkProc>,
    xess: u32,
    log: rsf_game_log_fn,
    log_user: *mut c_void,
) -> c_int {
    CONTROL
        .vulkan
        .creation()
        .begin(underlying, xess != 0, log, log_user)
        .into()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_creation_stop() {
    CONTROL.vulkan.creation().stop();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_xess_ready() -> c_int {
    CONTROL.vulkan.creation().xess_ready().into()
}

/// Read by `rsfg_vk_create` for the presenter host options.
#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_interop_ready() -> c_int {
    CONTROL.vulkan.creation().interop_ready().into()
}

/// # Safety
/// `name` is null or NUL-terminated for the call; `forwarded` is null or writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_creation_resolve(
    scope: u32,
    object: *mut c_void,
    name: *const c_char,
    forwarded: *mut Option<VkFunction>,
) -> u32 {
    // SAFETY: the caller's guarantee.
    let (route, function) = unsafe { CONTROL.vulkan.creation().resolve(scope, object, name) };
    if !forwarded.is_null() {
        // SAFETY: the caller's guarantee.
        unsafe { forwarded.write(function) };
    }
    route
}

/// # Safety
/// `leaves` is null or the creation table; `call` is null or a readable
/// `rsf_godot_vk_instance_call` whose record, request, allocator and output are live for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_create_instance(
    leaves: *const VkCreationLeaves,
    call: *const VkInstanceCall,
) -> i32 {
    // SAFETY: the caller's guarantee.
    match unsafe { (platform::creation_leaves(leaves), call.as_ref()) } {
        (Some(leaves), Some(call)) => CONTROL.vulkan.creation().create_instance(leaves, call),
        _ => VK_ERROR_INITIALIZATION_FAILED,
    }
}

/// # Safety
/// As `rsf_godot_vk_create_instance`, for an `rsf_godot_vk_device_call`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_create_device(
    leaves: *const VkCreationLeaves,
    call: *const VkDeviceCall,
) -> i32 {
    // SAFETY: the caller's guarantee.
    match unsafe { (platform::creation_leaves(leaves), call.as_ref()) } {
        (Some(leaves), Some(call)) => CONTROL.vulkan.creation().create_device(leaves, call),
        _ => VK_ERROR_INITIALIZATION_FAILED,
    }
}

/// # Safety
/// `leaves` is null or the creation table; `device` and `allocator` are the engine's arguments.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_destroy_device(
    leaves: *const VkCreationLeaves,
    device: *mut c_void,
    allocator: *const c_void,
) {
    // SAFETY: the caller's guarantee.
    if let Some(leaves) = unsafe { platform::creation_leaves(leaves) } {
        CONTROL
            .vulkan
            .creation()
            .destroy_device(leaves, device, allocator);
    }
}

// The input window owner (window.rs), called by the rsfg_* forwarders and the accept_window
// trampoline in runtime.cpp.

/// The Runtime's `accept_window` question and the admission `rsfg_vk_window` asks.
///
/// # Safety
/// `table` is null or the leaf table; `window` is a window handle, compared and passed through.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_accept_window(table: *const Platform, window: *mut c_void) -> c_int {
    // SAFETY: the caller's guarantee.
    unsafe { leaves(table) }
        .is_some_and(|platform| CONTROL.window.accept(platform, window))
        .into()
}

/// `rsfg_cpu`.
///
/// # Safety
/// `event` is null or a readable `rsf_game_cpu_event` of its
/// stated size.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_cpu(event: *const rsf_game_cpu_event) {
    // SAFETY: the caller's guarantee; the record is read only when its size covers it.
    let event = match unsafe { event.cast::<u32>().as_ref() } {
        Some(&size) if size as usize >= size_of::<rsf_game_cpu_event>() => unsafe { &*event },
        _ => return,
    };
    if let Some(platform) = platform::registered() {
        CONTROL.window.cpu(platform, event);
    }
}

/// The window part of `rsfg_shutdown`, before either backend stops.
///
/// # Safety
/// `table` is null or the leaf table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_window_release(table: *const Platform) {
    // SAFETY: the caller's guarantee.
    if let Some(platform) = unsafe { leaves(table) } {
        CONTROL.window.release(platform);
    }
}

/// Request a PCL ping for the admitted window.
#[unsafe(no_mangle)]
extern "C" fn rsfg_request_latency_ping() -> c_int {
    CONTROL.window.latency_ping()
}

/// `rsfg_vk_window`: while the Vulkan session is booted, asks the input window admission and
/// keeps its window as the session's primary window.
#[unsafe(no_mangle)]
extern "C" fn rsfg_vk_window(window: *mut c_void) {
    if let Some(platform) = platform::registered()
        && CONTROL.vulkan.booted()
    {
        CONTROL.window.accept(platform, window);
        CONTROL.vulkan.report_window(window);
    }
}

/// The Vulkan session's primary window, null when none was reported since its start. Read by
/// `rsfg_vk_create` under the guard.
#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_primary_window() -> *mut c_void {
    CONTROL.vulkan.primary_window()
}

/// `rsfg_vk_present_marker`: nonzero when the marker was reported for the admitted window.
///
/// # Safety
/// `table` is null or the leaf table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_present_marker(
    table: *const Platform,
    window: u64,
    begin: u32,
) -> c_int {
    // SAFETY: the caller's guarantee.
    unsafe { leaves(table) }
        .is_some_and(|platform| {
            CONTROL
                .window
                .present_marker(platform, CONTROL.vulkan.booted(), window, begin)
        })
        .into()
}

// The compositor effect owner (effect.rs), one per ReScaleFrameEffect, created and destroyed with
// the godot-cpp object.

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_effect_create() -> *mut Effect {
    Box::into_raw(Box::default())
}

/// # Safety
/// `effect` is null or an owner from `rsf_godot_effect_create` that no other call is using and
/// none uses afterwards.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_effect_destroy(effect: *mut Effect) {
    if !effect.is_null() {
        // SAFETY: the caller's guarantee; the owner came from Box::into_raw.
        drop(unsafe { Box::from_raw(effect) });
    }
}

/// `ReScaleFrameEffect.request_motion_capture`.
///
/// # Safety
/// `effect` is null or a live owner from `rsf_godot_effect_create`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_effect_request_capture(effect: *const Effect) {
    // SAFETY: the caller's guarantee.
    if let Some(effect) = unsafe { effect.as_ref() } {
        effect.request_capture();
    }
}

/// `ReScaleFrameEffect._render_callback` after its callback type check.
///
/// # Safety
/// `effect` is null or a live owner; `leaves` is null or the effect table; `call` is the
/// callback's record, live for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_effect_render(
    effect: *const Effect,
    leaves: *const EffectLeaves,
    call: *mut c_void,
) {
    // SAFETY: the caller's guarantee.
    if let (Some(effect), Some(leaves)) =
        unsafe { (effect.as_ref(), platform::effect_leaves(leaves)) }
    {
        effect.render(leaves, call);
    }
}

/// `ReScaleFrameEffect.get_status`.
///
/// # Safety
/// `effect` is null or a live owner; `status` is null or a writable `rsf_godot_effect_status` of
/// its stated size.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_effect_status_get(
    effect: *const Effect,
    status: *mut EffectStatus,
) -> c_int {
    // SAFETY: the caller's guarantee.
    match unsafe { (effect.as_ref(), checked_output(status, EFFECT_VERSION)) } {
        (Some(effect), Some(status)) => {
            // SAFETY: checked_output established the record.
            unsafe { status.write(effect.status()) };
            1
        }
        _ => 0,
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
