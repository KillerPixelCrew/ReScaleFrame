// SPDX-License-Identifier: MIT
//! Godot 4.7.2 native integration owner linked into `ReScaleFrame.Godot.Native.dll`. It owns the
//! Runtime control, settings and D3D12 session state (`control.rs`), the D3D12 SR context and
//! submission owner (`d3d12.rs`), the Vulkan session state (`vulkan.rs`) with its SR context
//! owner (`vulkan/sr.rs`) and device creation owner (`vulkan/creation.rs`), the input window, its procedure and the input session (`window.rs`),
//! the `ReScaleFrameRuntime` option schema and renderer admission (`extension.rs`) and each
//! `ReScaleFrameEffect`'s probe snapshot and motion capture classification (`effect.rs`).
//!
//! The GDExtension entry and godot-cpp classes stay C++. Native control uses the same C ABI.
//! `runtime.rs` handles the engine entry points, and `host.rs` implements the D3D12,
//! DXGI and Win32 leaves and calls the shared Runtime capture owner. `runtime.cpp`
//! wires the platform table, C header helpers and CMake-selected Vulkan capabilities.
//! `effect.cpp` lends godot-cpp leaves for each compositor callback.
mod control;
mod d3d12;
mod effect;
mod extension;
#[cfg(all(windows, not(test)))]
mod host;
#[cfg(all(windows, not(test)))]
mod host_abi;
mod platform;
#[cfg(all(windows, not(test)))]
mod runtime;
mod vulkan;
mod window;

use control::Control;
use effect::Effect;
use platform::{
    ABI_VERSION, EFFECT_VERSION, EffectLeaves, EffectStatus, EngineServices, OptionField,
    OptionValue, Platform, SETTINGS_ABI_VERSION, Settings, Setup, Status, VkFunction, VkProc,
};
use rsf_game_sys::{rsf_game_cpu_event, rsf_game_log_fn, rsf_game_render_pass};
use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::LazyLock;

static CONTROL: LazyLock<Control> = LazyLock::new(Control::new);

#[cfg(all(windows, not(test)))]
fn depth_domains_requested(domains: &rsf_game_sys::rsf_game_depth_domains) -> bool {
    use rsf_game_sys::{rsf_game_depth_domain, rsf_game_depth_domains};
    const {
        assert!(size_of::<rsf_game_depth_domain>() == 26 * 4);
        assert!(size_of::<rsf_game_depth_domains>() == 60 * 4);
    }
    // The C wire types contain only u32/f32 fields, with no padding. Any
    // nonzero byte requests mapping, including malformed records.
    unsafe {
        std::slice::from_raw_parts(
            std::ptr::from_ref(domains).cast::<u8>(),
            std::mem::size_of_val(domains),
        )
    }
    .iter()
    .any(|byte| *byte != 0)
}

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

// The Vulkan session binds the CPU services at boot and releases them after checked shutdown.

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

use vulkan::creation::typed::{self, Requirements};

/// On supported builds the caller lends the linked Runtime requirements table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_creation_resolver(
    underlying: Option<VkProc>,
    xess: u32,
    log: rsf_game_log_fn,
    log_user: *mut c_void,
    requirements: *const Requirements,
) -> Option<VkProc> {
    if !unsafe { typed::bind_requirements(requirements) } {
        return None;
    }
    CONTROL
        .vulkan
        .creation()
        .begin(underlying, xess != 0, log, log_user)
        .then_some(vk_instance_proc as VkProc)
}

unsafe extern "C" fn vk_instance_proc(
    object: *mut c_void,
    name: *const c_char,
) -> Option<VkFunction> {
    unsafe { vk_resolve(platform::VK_SCOPE_INSTANCE, object, name) }
}

unsafe extern "C" fn vk_device_proc(
    object: *mut c_void,
    name: *const c_char,
) -> Option<VkFunction> {
    unsafe { vk_resolve(platform::VK_SCOPE_DEVICE, object, name) }
}

unsafe fn vk_resolve(scope: u32, object: *mut c_void, name: *const c_char) -> Option<VkFunction> {
    let (route, forwarded) = unsafe { CONTROL.vulkan.creation().resolve(scope, object, name) };
    match route {
        platform::VK_ROUTE_FORWARD => forwarded,
        platform::VK_ROUTE_DEVICE_PROC => {
            let proc: VkProc = vk_device_proc;
            // Vulkan's resolver returns a generic PFN_vkVoidFunction.
            Some(unsafe { std::mem::transmute::<VkProc, VkFunction>(proc) })
        }
        _ => typed::entry(route),
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

/// # Safety
/// The native export checked the pass size and borrows it for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_execution(pass: *const rsf_game_render_pass, begin: u32) -> c_int {
    // SAFETY: the native export lends a complete pass.
    let pass = unsafe { &*pass };
    CONTROL.window.execution(pass, begin).into()
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

/// Compose the options from a borrowed Godot Dictionary, then call the registered
/// engine service with the typed settings. Only the leaf can interpret Variants.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_extension_configure(
    table: *const Platform,
    admitted: u32,
    viewport: u64,
    dictionary: *const c_void,
    read: unsafe extern "C" fn(*const c_void, *const OptionField, *mut OptionValue) -> u32,
) -> c_int {
    let required = extension::ADMIT_DRIVER | extension::ADMIT_RENDERER;
    if admitted & required != required {
        return 0;
    }
    let mut values = [OptionValue::default(); extension::FIELD_COUNT];
    for (field, value) in extension::FIELDS.0.iter().zip(&mut values) {
        if unsafe { read(dictionary, field, value) } == 0 {
            return 0;
        }
    }
    let Some(platform) = (unsafe { leaves(table) }) else {
        return 0;
    };
    extension::compose(platform, &values)
        .map_or(0, |value| CONTROL.configure_engine(&value, viewport))
}

/// The first bit admits a status snapshot; the second admits requested settings.
/// The C++ class only materializes the corresponding Dictionary entries.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_extension_status(
    status: *mut Status,
    settings: *mut Settings,
) -> u32 {
    let mut current = Status {
        struct_size: size_of::<Status>() as u32,
        abi_version: ABI_VERSION,
        ..Default::default()
    };
    if unsafe { rsfg_status(&mut current) } == 0 || current.session_id == 0 {
        return 0;
    }
    unsafe { status.write(current) };

    let mut requested = Settings {
        struct_size: size_of::<Settings>() as u32,
        abi_version: SETTINGS_ABI_VERSION,
        ..Default::default()
    };
    if unsafe { rsfg_settings(&mut requested) } == 0 {
        return 1;
    }
    unsafe { settings.write(requested) };
    3
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
