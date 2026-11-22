// SPDX-License-Identifier: MIT
//! Skyrim's guarded pre-HDR reconstruction and temporary native TAA bypass.

use crate::compatibility::HdrCall;
use crate::platform::{self, Platform};
use crate::session::{rsf_skyrim_session_accepting, rsf_skyrim_session_lease};
use crate::taa;
use rsf_game_sys::rsf_game_host_services;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

#[repr(C)]
struct Leaves {
    render_allowed: unsafe extern "C" fn() -> bool,
    matches_call: unsafe extern "C" fn(&HdrCall) -> bool,
    fg_inputs: unsafe extern "C" fn(*mut c_void, &rsf_game_host_services),
    reconstruct: unsafe extern "C" fn(*mut c_void, &rsf_game_host_services) -> bool,
    observe: unsafe extern "C" fn(&HdrCall, &rsf_game_host_services, u64, bool),
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TaaReceipt {
    manager: usize,
    taa: usize,
    enabled: u8,
}

const _: () = {
    assert!(size_of::<Leaves>() == 40);
    assert!(size_of::<TaaReceipt>() == 24 && std::mem::offset_of!(TaaReceipt, enabled) == 16);
};

static OBSERVATIONS: AtomicU64 = AtomicU64::new(0);

fn services() -> rsf_game_host_services {
    // The C record consists of numeric fields, raw pointers and nullable callbacks.
    let mut host = unsafe { std::mem::zeroed() };
    unsafe { rsf_skyrim_session_lease(true, &mut host) };
    host
}

enum Restore {
    Idle,
    Bypassing,
    Pending(TaaReceipt),
}

static RESTORE: Mutex<Restore> = Mutex::new(Restore::Idle);

fn drain(platform: &Platform, state: &mut Restore) -> bool {
    match state {
        Restore::Idle => return true,
        Restore::Bypassing => return false,
        Restore::Pending(receipt) => {
            let Some(current) = taa::owner(platform, receipt.manager) else {
                return false;
            };
            if current == receipt.taa
                && !taa::write(platform, receipt.taa + taa::ENABLED, &[receipt.enabled])
            {
                return false;
            }
        }
    }
    *state = Restore::Idle;
    true
}

fn bypass(platform: &Platform, manager: usize) -> Option<TaaReceipt> {
    let mut state = RESTORE.lock().unwrap_or_else(PoisonError::into_inner);
    if !drain(platform, &mut state) {
        return None;
    }
    let taa = taa::owner(platform, manager).filter(|&taa| taa != 0)?;
    let enabled_address = taa.checked_add(taa::ENABLED)?;
    let mut enabled = [0; 1];
    if !platform.read(enabled_address, &mut enabled)
        || taa::clear_main_history(platform, taa).is_none()
        || !taa::write(platform, enabled_address, &[0])
    {
        return None;
    }
    *state = Restore::Bypassing;
    Some(TaaReceipt {
        manager,
        taa,
        enabled: enabled[0],
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_postprocess_busy(platform: *const Platform) -> bool {
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return true;
    };
    !drain(
        platform,
        &mut RESTORE.lock().unwrap_or_else(PoisonError::into_inner),
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_postprocess_prepare(
    platform: *const Platform,
    leaves: &Leaves,
    call: &HdrCall,
    receipt: &mut TaaReceipt,
) {
    *receipt = TaaReceipt::default();
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return;
    };
    if !drain(
        platform,
        &mut RESTORE.lock().unwrap_or_else(PoisonError::into_inner),
    ) {
        return;
    }
    let mut reconstructed = false;
    if rsf_skyrim_session_accepting()
        && unsafe { (leaves.render_allowed)() }
        && call.input == 1
        && call.output == 0
        && unsafe { (leaves.matches_call)(call) }
    {
        let host = services();
        let manager = ptr::with_exposed_provenance_mut(call.manager);
        // Pre-mutation FG facts must reach the Runtime before SR can write these resources.
        unsafe { (leaves.fg_inputs)(manager, &host) };
        reconstructed = unsafe { (leaves.reconstruct)(manager, &host) };
    }
    if rsf_skyrim_session_accepting() {
        let number = OBSERVATIONS.fetch_add(1, Ordering::Relaxed);
        if number < 16 {
            let host = services();
            if host.log.is_some() {
                unsafe { (leaves.observe)(call, &host, number, number < 4) };
            }
        }
    }
    if reconstructed {
        *receipt = bypass(platform, call.manager).unwrap_or_default();
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_postprocess_restore(
    platform: *const Platform,
    receipt: &TaaReceipt,
) {
    if receipt.taa == 0 {
        return;
    }
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return;
    };
    let restored = {
        let mut state = RESTORE.lock().unwrap_or_else(PoisonError::into_inner);
        *state = Restore::Pending(*receipt);
        drain(platform, &mut state)
    };
    if !restored {
        platform.log(
            &services(),
            c"Skyrim native TAA restore pending after guarded restore refusal.",
        );
    }
}
