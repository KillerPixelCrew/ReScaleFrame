// SPDX-License-Identifier: MIT
//! Native renderer jitter, frame-upload and end-of-render observations for Skyrim.

use crate::layout::{ACTIVE_WINDOW, NATIVE_FRAME};
use crate::platform::{self, Platform};
use crate::render_native::rsf_skyrim_render_allowed;
use crate::session::{rsf_skyrim_session_accepting, rsf_skyrim_session_lease};
use crate::{auxiliary, presentation, resolution, sampling};
use rsf_game_sys::rsf_game_host_services;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use winapi::um::winuser::GetClassNameW;

const SWAPCHAIN: usize = 0x18;
const RENDERER: usize = 0x333_1f40;
const UPLOAD_DIRTY: usize = 0x333_6400;
const JITTER_OBSERVATIONS: u64 = 16;
const WINDOW_CLASS: &[u8] = b"Skyrim Special Edition";

#[repr(C)]
struct PresentationBinding {
    struct_size: u32,
    abi_version: u32,
    swapchain: *mut c_void,
}

const _: () = assert!(size_of::<PresentationBinding>() == 16);

type Bind = unsafe extern "C" fn(*mut c_void, &PresentationBinding) -> i32;

static OBSERVATIONS: AtomicU64 = AtomicU64::new(0);

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_skyrim_reset_render_observations() {
    OBSERVATIONS.store(0, Ordering::Relaxed);
    crate::postprocess::reset_observations();
}

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
unsafe extern "C" fn rsf_skyrim_render_end_enter(retained: &mut rsf_game_host_services) {
    let binding = unsafe { rsf_skyrim_session_lease(false, retained) };
    if rsf_skyrim_session_accepting()
        && unsafe { rsf_skyrim_render_allowed() }
        && binding != 0
        && let Some(platform) = platform::get()
        && let Some(chain) = swapchain(platform)
    {
        let request = PresentationBinding {
            struct_size: size_of::<PresentationBinding>() as u32,
            abi_version: platform.d3d11_host_abi,
            swapchain: ptr::with_exposed_provenance_mut(chain),
        };
        let bind: Bind = unsafe { std::mem::transmute(binding) };
        unsafe { bind(retained.user, &request) };
    }
    sampling::finish();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_render_end_leave(retained: &rsf_game_host_services) {
    presentation::finish();
    if let Some(platform) = platform::get() {
        auxiliary::finish(platform, retained);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_render_upload_enter(
    renderer: usize,
    observation: &mut UploadObservation,
) -> bool {
    let Some(platform) = platform::get() else {
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

const _: () = assert!(
    size_of::<UploadObservation>() == 16 && std::mem::offset_of!(UploadObservation, frame) == 8
);

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
extern "C" fn rsf_skyrim_render_jitter_observed(state: usize) {
    // A withdrawn or incompatible host still gives the resolution owner a chance to restore.
    let mut host: rsf_game_host_services = unsafe { std::mem::zeroed() };
    unsafe { rsf_skyrim_session_lease(true, &mut host) };
    if !unsafe { rsf_skyrim_render_allowed() } {
        host = unsafe { std::mem::zeroed() };
    }
    let Some(platform) = platform::get() else {
        return;
    };
    let _ = resolution::configure(platform, state, &host);
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
    let dimensions = if state == 0 {
        None
    } else {
        pair(platform, state, 0x24)
    };
    let projection = dimensions.and_then(|_| pair(platform, state, 0x44));
    let [width, height] = dimensions.unwrap_or_default();
    let [x, y] = projection.unwrap_or_default().map(f32::from_bits);
    let readable = u8::from(projection.is_some());
    platform.log_text(&host, &format!(
        "Skyrim native jitter observation={number} state=0x{state:x} readable={readable} size={width}x{height} projection={x},{y}",
    ));
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_render_accept_window(window: *mut c_void) -> bool {
    if window.is_null() {
        return false;
    }
    let mut name = [0; 128];
    let len =
        unsafe { GetClassNameW(window.cast(), name.as_mut_ptr(), name.len() as i32) } as usize;
    len == WINDOW_CLASS.len()
        && name[..len]
            .iter()
            .zip(WINDOW_CLASS)
            .all(|(&wide, &ascii)| wide == u16::from(ascii))
}
