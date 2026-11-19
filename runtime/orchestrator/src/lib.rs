// SPDX-License-Identifier: MIT

#[cfg(windows)]
mod cpu_abi;
#[cfg(windows)]
mod cpu_reservations;

#[cfg(windows)]
mod fg_choice;
#[cfg(windows)]
mod fg_dispatch;
#[cfg(windows)]
mod fg_leases;
#[cfg(windows)]
mod fg_session;
#[cfg(windows)]
mod frame_assembly;
mod frame_sequencer;
#[cfg(windows)]
mod native_scene;
#[cfg(windows)]
mod native_window;
#[cfg(windows)]
mod plugin_session;
#[cfg(windows)]
mod render_abi;
#[cfg(windows)]
mod render_links;
#[cfg(windows)]
mod scene_abi;
mod sr_history;
#[cfg(windows)]
mod sr_session;

// C callers can recover from allocation failure. A successful handle is
// released with Box::from_raw after its owner has retired outstanding users.
fn allocate_handle<T>(value: T) -> *mut T {
    const { assert!(size_of::<T>() != 0) }
    let raw = unsafe { std::alloc::alloc(std::alloc::Layout::new::<T>()).cast::<T>() };
    if !raw.is_null() {
        unsafe { raw.write(value) };
    }
    raw
}

// Public backend IDs from rescaleframe/sr_session.h, checked by the native ABI test.
const NONE: u32 = 0;
const DLSS: u32 = 1;
const FSR3: u32 = 3;
const XESS: u32 = 5;

#[unsafe(no_mangle)]
pub extern "C" fn rsf_gpu_default_sr_backend(vendor: u32, software: u32) -> u32 {
    if software != 0 {
        return NONE;
    }
    match vendor {
        0x10de => DLSS,
        0x8086 => XESS,
        _ => FSR3,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rsf_gpu_select_sr_backend(vendor: u32, software: u32, choices: u32) -> u32 {
    let preferred = rsf_gpu_default_sr_backend(vendor, software);
    if preferred == NONE {
        return NONE;
    }
    [preferred, FSR3, XESS]
        .into_iter()
        .find(|backend| choices & (1 << backend) != 0)
        .unwrap_or(NONE)
}
