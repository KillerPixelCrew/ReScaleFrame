// SPDX-License-Identifier: MIT
//! Test leaves. Recorded calls are thread-local, so parallel tests do not share them.
use super::{
    FgFacts, FgInstall, FgOptions, InputFacts, LatencyFacts, PLATFORM_VERSION, Platform, Settings,
    Sharpening, Status, WindowFacts,
};
use std::cell::Cell;

thread_local! {
    pub(crate) static INSTALL_RESULT: Cell<i32> = const { Cell::new(0) };
    pub(crate) static INSTALLS: Cell<u32> = const { Cell::new(0) };
    pub(crate) static FG_OPTIONS: Cell<Option<FgOptions>> = const { Cell::new(None) };
    pub(crate) static FG: Cell<FgFacts> = Cell::new(FgFacts::default());
    pub(crate) static LATENCY: Cell<Option<LatencyFacts>> = const { Cell::new(None) };
}

/// `rsf_sharpening_defaults()` as `sharpening_settings.h` defines it.
pub(crate) fn sharpening_defaults() -> Sharpening {
    Sharpening {
        struct_size: size_of::<Sharpening>() as u32,
        abi_version: 1,
        mode: 0,
        sharpness: 0.4,
        mas: 0,
        motion_sharpness: 0.2,
        motion_threshold: 0.0,
        motion_scale_limit: 10.0,
        contrast_enabled: 0,
        contrast: 0.0,
        depth_scale: 35.0,
        depth_bias: 0.001,
        clamp_output: 0,
        debug: 0,
    }
}

unsafe extern "C" fn tick_count() -> u64 {
    41
}

unsafe extern "C" fn prepare_capture() -> u32 {
    1
}

unsafe extern "C" fn fg_install(_: *const FgInstall) -> i32 {
    INSTALLS.set(INSTALLS.get() + 1);
    INSTALL_RESULT.get()
}

unsafe extern "C" fn fg_options(options: *const FgOptions) {
    // SAFETY: the owner passes a live record.
    FG_OPTIONS.set(Some(unsafe { *options }));
}

unsafe extern "C" fn dlss_preset_valid(preset: u32) -> u32 {
    u32::from(preset <= 15)
}

unsafe extern "C" fn sharpening_valid(options: *const Sharpening) -> u32 {
    // SAFETY: the owner passes a live record.
    u32::from(unsafe { (*options).mode } <= 4)
}

unsafe extern "C" fn write_sharpening_defaults(options: *mut Sharpening) {
    // SAFETY: the owner passes a writable record.
    unsafe { options.write(sharpening_defaults()) };
}

unsafe extern "C" fn fg_backend_known(backend: u32) -> u32 {
    u32::from(matches!(backend, 1 | 3 | 4 | 5))
}

unsafe extern "C" fn vulkan_off() -> u32 {
    0
}

unsafe extern "C" fn vulkan_on() -> u32 {
    1
}

pub(crate) const VULKAN_CONFIGURED: i32 = 7;

unsafe extern "C" fn vulkan_configure(_: *const Settings) -> i32 {
    VULKAN_CONFIGURED
}

unsafe extern "C" fn vulkan_settings(_: *mut Settings) -> i32 {
    1
}

pub(crate) const VULKAN_SESSION: u64 = 99;

unsafe extern "C" fn vulkan_status(status: *mut Status) -> i32 {
    // SAFETY: the owner passes a checked record.
    unsafe { (*status).session_id = VULKAN_SESSION };
    1
}

pub(crate) const INPUTS: InputFacts = InputFacts {
    keyboard: 1,
    mouse: 2,
    controller: 3,
    pings: 4,
    substitutes: 5,
    order_violations: 6,
    derived_boundaries: 7,
};

unsafe extern "C" fn cpu_counters(facts: *mut InputFacts) {
    // SAFETY: the owner passes a writable record.
    unsafe { facts.write(INPUTS) };
}

unsafe extern "C" fn window_facts(facts: *mut WindowFacts) {
    // SAFETY: the owner passes a writable record.
    unsafe {
        facts.write(WindowFacts {
            foreground_process_id: 10,
            current_process_id: 10,
            input_window_visible: 1,
            reserved: 0,
        })
    };
}

unsafe extern "C" fn fg_facts(facts: *mut FgFacts) {
    // SAFETY: the owner passes a writable record.
    unsafe { facts.write(FG.get()) };
}

unsafe extern "C" fn latency_facts(facts: *mut LatencyFacts) -> u32 {
    match LATENCY.get() {
        Some(value) => {
            // SAFETY: the owner passes a writable record.
            unsafe { facts.write(value) };
            1
        }
        None => 0,
    }
}

const fn table(vulkan: bool) -> Platform {
    let vulkan_booted: unsafe extern "C" fn() -> u32 = if vulkan { vulkan_on } else { vulkan_off };
    Platform {
        struct_size: size_of::<Platform>() as u32,
        version: PLATFORM_VERSION,
        tick_count,
        prepare_capture,
        fg_install,
        fg_options,
        dlss_preset_valid,
        sharpening_valid,
        sharpening_defaults: write_sharpening_defaults,
        fg_backend_known,
        vulkan_booted,
        vulkan_configure,
        vulkan_settings,
        vulkan_status,
        cpu_counters,
        window_facts,
        fg_facts,
        latency_facts,
    }
}

pub(crate) static D3D12: Platform = table(false);
pub(crate) static VULKAN: Platform = table(true);
