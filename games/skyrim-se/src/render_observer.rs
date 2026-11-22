// SPDX-License-Identifier: MIT
//! Native renderer jitter, frame-upload and end-of-render observations for Skyrim.

use crate::platform::{self, Platform};
use crate::session::{rsf_skyrim_session_accepting, rsf_skyrim_session_lease};
use crate::{presentation, resolution, sampling};
use rsf_game_sys::rsf_game_host_services;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

const ACTIVE_WINDOW: usize = 0x333_0198;
const SWAPCHAIN: usize = 0x18;
const RENDERER: usize = 0x333_1f40;
const NATIVE_FRAME: usize = 0x333_63f4;
const UPLOAD_DIRTY: usize = 0x333_6400;
const JITTER_OBSERVATIONS: u64 = 16;
const WINDOW_CLASS: &[u8] = b"Skyrim Special Edition";

#[repr(C)]
struct PresentationBinding {
    struct_size: u32,
    abi_version: u32,
    swapchain: *mut c_void,
}

#[repr(C)]
struct JitterReport {
    state: usize,
    number: u64,
    dimensions: [u32; 2],
    projection: [f32; 2],
    readable: bool,
}

#[repr(C)]
struct Leaves {
    render_allowed: unsafe extern "C" fn() -> bool,
    bind: unsafe extern "C" fn(usize, *mut c_void, &PresentationBinding),
    configure_resolution: unsafe extern "C" fn(*mut c_void, &rsf_game_host_services),
    finish_auxiliaries: unsafe extern "C" fn(&rsf_game_host_services),
    report_jitter: unsafe extern "C" fn(&rsf_game_host_services, &JitterReport),
    window_class: unsafe extern "C" fn(*mut c_void, *mut u16, u32) -> u32,
}

const _: () = {
    assert!(size_of::<PresentationBinding>() == 16);
    assert!(size_of::<JitterReport>() == 40 && std::mem::offset_of!(JitterReport, readable) == 32);
    assert!(size_of::<Leaves>() == 48);
};

static OBSERVATIONS: AtomicU64 = AtomicU64::new(0);

fn swapchain(platform: &Platform) -> Option<usize> {
    let base = platform.main_module();
    if base == 0 {
        return None;
    }
    let window = platform.read_usize(base + ACTIVE_WINDOW)?;
    if window == 0 {
        return None;
    }
    platform
        .read_usize(window.checked_add(SWAPCHAIN)?)
        .filter(|&chain| chain != 0)
}

/// The native detour holds this services record through its original renderer-end call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_render_end_enter(
    platform: *const Platform,
    leaves: &Leaves,
    retained: &mut rsf_game_host_services,
) {
    let binding = unsafe { rsf_skyrim_session_lease(false, retained) };
    if rsf_skyrim_session_accepting()
        && unsafe { (leaves.render_allowed)() }
        && binding != 0
        && let Some(platform) = unsafe { platform::inspect(platform) }
        && let Some(chain) = swapchain(platform)
    {
        let request = PresentationBinding {
            struct_size: size_of::<PresentationBinding>() as u32,
            abi_version: platform.d3d11_host_abi,
            swapchain: ptr::with_exposed_provenance_mut(chain),
        };
        unsafe { (leaves.bind)(binding, retained.user, &request) };
    }
    sampling::finish();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_render_end_leave(leaves: &Leaves, retained: &rsf_game_host_services) {
    presentation::finish();
    unsafe { (leaves.finish_auxiliaries)(retained) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_render_upload_enter(
    platform: *const Platform,
    renderer: usize,
    observation: &mut UploadObservation,
) -> bool {
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return false;
    };
    let base = platform.main_module();
    let mut dirty = [0];
    if base == 0
        || base.checked_add(RENDERER) != Some(renderer)
        || !platform.read(base + UPLOAD_DIRTY, &mut dirty)
        || dirty[0] == 0
        || !rsf_skyrim_session_accepting()
    {
        return false;
    }
    let mut host: rsf_game_host_services = unsafe { std::mem::zeroed() };
    unsafe { rsf_skyrim_session_lease(true, &mut host) };
    let Some(frame) = platform.read_u32(base + NATIVE_FRAME) else {
        return false;
    };
    if host.session_id == 0 {
        return false;
    }
    *observation = UploadObservation {
        session: host.session_id,
        frame,
    };
    true
}

#[repr(C)]
struct UploadObservation {
    session: u64,
    frame: u32,
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_render_upload_leave(observation: &UploadObservation, flags: u8) {
    resolution::record_per_frame_flags(observation.session, observation.frame, flags);
}

fn pair(platform: &Platform, state: usize, offset: usize) -> Option<[u32; 2]> {
    let mut words = [[0; 4]; 2];
    platform
        .read(state.checked_add(offset)?, words.as_flattened_mut())
        .then_some(words.map(u32::from_le_bytes))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_render_jitter_observed(
    platform: *const Platform,
    leaves: &Leaves,
    state: usize,
) {
    // A withdrawn or incompatible host still gives the resolution owner a chance to restore.
    let mut host: rsf_game_host_services = unsafe { std::mem::zeroed() };
    unsafe { rsf_skyrim_session_lease(true, &mut host) };
    if !unsafe { (leaves.render_allowed)() } {
        host = unsafe { std::mem::zeroed() };
    }
    unsafe { (leaves.configure_resolution)(ptr::with_exposed_provenance_mut(state), &host) };
    if !rsf_skyrim_session_accepting() {
        return;
    }
    let number = OBSERVATIONS.fetch_add(1, Ordering::Relaxed);
    if number >= JITTER_OBSERVATIONS {
        return;
    }
    unsafe { rsf_skyrim_session_lease(true, &mut host) };
    if host.log.is_none() {
        return;
    }
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return;
    };
    let dimensions = if state == 0 {
        None
    } else {
        pair(platform, state, 0x24)
    };
    let projection = dimensions.and_then(|_| pair(platform, state, 0x44));
    let observation = JitterReport {
        state,
        number,
        dimensions: dimensions.unwrap_or_default(),
        projection: projection.unwrap_or_default().map(f32::from_bits),
        readable: projection.is_some(),
    };
    unsafe { (leaves.report_jitter)(&host, &observation) };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_render_accept_window(leaves: &Leaves, window: *mut c_void) -> bool {
    if window.is_null() {
        return false;
    }
    let mut name = [0; 128];
    let len =
        unsafe { (leaves.window_class)(window, name.as_mut_ptr(), name.len() as u32) } as usize;
    len == WINDOW_CLASS.len()
        && name[..len]
            .iter()
            .zip(WINDOW_CLASS)
            .all(|(&wide, &ascii)| wide == u16::from(ascii))
}
