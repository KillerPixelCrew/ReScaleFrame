// SPDX-License-Identifier: MIT
//! Skyrim's guarded pre-HDR reconstruction and temporary native TAA bypass.

use crate::compatibility::HdrCall;
use crate::platform::{self, Platform};
use crate::session::{rsf_skyrim_session_accepting, rsf_skyrim_session_lease};
use rsf_game_sys::rsf_game_host_services;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

const TAA_OWNER: usize = 0x1f0;
const TAA_ENABLED: usize = 0x18;
const TAA_CLEAR_MASK: usize = 0x30;
const CLEAR_MAIN_HISTORY: u32 = 1;

#[repr(C)]
struct Leaves {
    render_allowed: unsafe extern "C" fn() -> bool,
    matches_call: unsafe extern "C" fn(&HdrCall) -> bool,
    fg_inputs: unsafe extern "C" fn(*mut c_void, &rsf_game_host_services),
    reconstruct: unsafe extern "C" fn(*mut c_void, &rsf_game_host_services) -> bool,
    observe: unsafe extern "C" fn(&HdrCall, &rsf_game_host_services, u64, bool),
    write: unsafe extern "C" fn(usize, *const c_void, usize) -> bool,
}

#[repr(C)]
#[derive(Default)]
struct TaaReceipt {
    manager: usize,
    taa: usize,
    enabled: u8,
}

const _: () = {
    assert!(size_of::<Leaves>() == 48);
    assert!(size_of::<TaaReceipt>() == 24 && std::mem::offset_of!(TaaReceipt, enabled) == 16);
};

static OBSERVATIONS: AtomicU64 = AtomicU64::new(0);

fn services() -> rsf_game_host_services {
    // The C record consists of numeric fields, raw pointers and nullable callbacks.
    let mut host = unsafe { std::mem::zeroed() };
    unsafe { rsf_skyrim_session_lease(true, &mut host) };
    host
}

fn taa_owner(platform: &Platform, manager: usize) -> Option<usize> {
    if manager == 0 {
        return None;
    }
    platform
        .read_usize(manager.checked_add(TAA_OWNER)?)
        .filter(|&taa| taa != 0)
}

fn bypass(platform: &Platform, leaves: &Leaves, manager: usize) -> Option<TaaReceipt> {
    let taa = taa_owner(platform, manager)?;
    let enabled_address = taa.checked_add(TAA_ENABLED)?;
    let mask_address = taa.checked_add(TAA_CLEAR_MASK)?;
    let mut enabled = [0; 1];
    let mut mask = [0; 4];
    if !platform.read(enabled_address, &mut enabled) || !platform.read(mask_address, &mut mask) {
        return None;
    }
    // The native TAA reset consumes bit zero for main history targets 81 and 82.
    let clear = (u32::from_le_bytes(mask) | CLEAR_MAIN_HISTORY).to_le_bytes();
    if !unsafe { (leaves.write)(mask_address, clear.as_ptr().cast(), clear.len()) }
        || !unsafe { (leaves.write)(enabled_address, ptr::from_ref(&0_u8).cast(), 1) }
    {
        return None;
    }
    Some(TaaReceipt {
        manager,
        taa,
        enabled: enabled[0],
    })
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
        *receipt = bypass(platform, leaves, call.manager).unwrap_or_default();
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_postprocess_restore(
    platform: *const Platform,
    leaves: &Leaves,
    receipt: &TaaReceipt,
) {
    if receipt.taa == 0 {
        return;
    }
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return;
    };
    if taa_owner(platform, receipt.manager) == Some(receipt.taa) {
        unsafe {
            (leaves.write)(
                receipt.taa + TAA_ENABLED,
                ptr::from_ref(&receipt.enabled).cast(),
                1,
            )
        };
    }
}
