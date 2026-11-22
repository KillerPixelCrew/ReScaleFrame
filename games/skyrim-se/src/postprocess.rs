// SPDX-License-Identifier: MIT
//! Skyrim's guarded pre-HDR reconstruction and temporary native TAA bypass.

use crate::compatibility::{HdrCall, matches_postprocess, render_allowed};
use crate::cpu_events::render_source;
use crate::layout::target_slot;
use crate::platform::{self, Platform};
use crate::render_inputs;
use crate::render_native::thread;
use crate::session;
use crate::taa;
use rsf_game_sys::rsf_game_host_services;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use winapi::um::d3d11::ID3D11Texture2D;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TaaReceipt {
    manager: usize,
    taa: usize,
    enabled: u8,
}

const _: () = {
    assert!(size_of::<TaaReceipt>() == 24 && std::mem::offset_of!(TaaReceipt, enabled) == 16);
};

static OBSERVATIONS: AtomicU64 = AtomicU64::new(0);

fn services() -> rsf_game_host_services {
    // The C record consists of numeric fields, raw pointers and nullable callbacks.
    let mut host = unsafe { std::mem::zeroed() };
    unsafe { session::lease(true, &mut host) };
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

pub(crate) fn reset_observations() {
    OBSERVATIONS.store(0, Ordering::Relaxed);
    crate::camera::render::reset_history();
}

pub(crate) fn cleanup_pending() -> bool {
    let Some(platform) = platform::get() else {
        return crate::auxiliary::pending();
    };
    crate::auxiliary::pending()
        || !drain(
            platform,
            &mut RESTORE.lock().unwrap_or_else(PoisonError::into_inner),
        )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_postprocess_prepare(call: &HdrCall, receipt: &mut TaaReceipt) {
    *receipt = TaaReceipt::default();
    let Some(platform) = platform::get() else {
        return;
    };
    if !drain(
        platform,
        &mut RESTORE.lock().unwrap_or_else(PoisonError::into_inner),
    ) {
        return;
    }
    let mut reconstructed = false;
    if session::accepting()
        && render_allowed()
        && call.input == 1
        && call.output == 0
        && matches_postprocess(call)
    {
        let host = services();
        // Pre-mutation FG facts must reach the Runtime before SR can write these resources.
        let _ = render_inputs::produce(platform, call.manager, &host, true);
        reconstructed =
            render_inputs::produce(platform, call.manager, &host, false).unwrap_or(false);
    }
    if session::accepting() {
        let number = OBSERVATIONS.fetch_add(1, Ordering::Relaxed);
        if number < 16 {
            let host = services();
            if host.log.is_some() {
                report_postprocess(platform, call, &host, number);
            }
        }
    }
    if reconstructed {
        *receipt = bypass(platform, call.manager).unwrap_or_default();
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_postprocess_restore(receipt: &TaaReceipt) {
    if receipt.taa == 0 {
        return;
    }
    let Some(platform) = platform::get() else {
        return;
    };
    let restored = {
        let mut state = RESTORE.lock().unwrap_or_else(PoisonError::into_inner);
        *state = Restore::Pending(*receipt);
        drain(platform, &mut state)
    };
    if !restored {
        let mut retained: rsf_game_host_services = unsafe { std::mem::zeroed() };
        unsafe { session::lease(false, &mut retained) };
        platform.log(
            &retained,
            c"Skyrim native TAA restore pending after guarded restore refusal.",
        );
    }
}

fn describe_target(platform: &Platform, base: usize, index: u32) -> String {
    if base != 0
        && matches!(index, 1 | 7)
        && let Some(texture) = platform.read_usize(target_slot(base, index as usize))
        && texture != 0
    {
        // The synchronous postprocess callback owns these researched slots.
        let texture = ptr::with_exposed_provenance_mut::<ID3D11Texture2D>(texture);
        let mut desc = unsafe { std::mem::zeroed() };
        unsafe { (*texture).GetDesc(&mut desc) };
        return format!(
            "target={index} texture={texture:p} size={}x{} format={} samples={} bind=0x{:x}",
            desc.Width, desc.Height, desc.Format, desc.SampleDesc.Count, desc.BindFlags,
        );
    }
    format!("target={index} unavailable")
}

fn report_postprocess(
    platform: &Platform,
    call: &HdrCall,
    host: &rsf_game_host_services,
    number: u64,
) {
    let base = platform.main_module();
    let colour = describe_target(platform, base, call.input);
    let motion = describe_target(platform, base, 7);
    let source = render_source();
    platform.log_text(host, &format!(
        "Skyrim native post-process observation={number} source={source} thread={} manager=0x{:x} input={} output={}; {colour}; motion candidate {motion}",
        thread(), call.manager, call.input, call.output,
    ));
    if number < 4 && base != 0 {
        crate::camera::render::describe_inputs(platform, base, host);
    }
}
