// SPDX-License-Identifier: MIT
//! Owns Skyrim's engine render ratios, jitter and temporary output viewport.

use crate::layout::{ACTIVE_WINDOW, NATIVE_FRAME};
use crate::platform::Platform;
use crate::render_inputs::renderer_thread;
use crate::render_native::{
    ComRef, rsf_skyrim_call_render_config, rsf_skyrim_call_upscale, thread,
};
use crate::taa;
use rsf_game_sys::sampling::halton_f32 as halton;
use rsf_game_sys::{
    RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_RECONFIGURE, RSF_GAME_RENDER_SR, rsf_game_host_services,
    rsf_game_render_config, rsf_game_render_pass,
};
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Mutex, MutexGuard, PoisonError};
use winapi::shared::dxgi::IDXGISwapChain;
use winapi::um::d3d11::ID3D11DeviceContext;

const RENDER_STATE: usize = 0x333_63a0;
const RENDERER: usize = 0x333_1f40;
const RATIO: usize = 0x114;
const RATIO_LOCK: usize = 0x128;
const JITTER: usize = 0x44;
const UPLOAD_DIRTY: usize = 0x333_6400;
const UPLOAD: usize = 0x100_ae80;
const VIEWPORT: usize = 0x100_9960;
const TAA_MANAGER: usize = 0x33d_40a0;
const IMMEDIATE_CONTEXT: usize = 0x333_1f30;

#[derive(Clone, Copy)]
struct Owner {
    pointer: usize,
    session: u64,
    output: [u32; 2],
    render: [u32; 2],
    saved_ratios: [f32; 4],
    saved_lock: u32,
    last_ratios: [f32; 2],
}

#[derive(Clone, Copy)]
struct Output {
    producer_ratios: [f32; 4],
    thread: u32,
    flags: u8,
}

struct State {
    owner: Option<Owner>,
    output: Option<Output>,
}

static STATE: Mutex<State> = Mutex::new(State {
    owner: None,
    output: None,
});
// Serializes mutations. Reentry from the engine or Runtime cannot wait on its own call.
static MUTATION: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy)]
struct FrameFlags {
    session: u64,
    frame: u32,
    flags: u8,
}

thread_local! {
    static FRAME_FLAGS: Cell<Option<FrameFlags>> = const { Cell::new(None) };
}

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn words<const N: usize>(platform: &Platform, address: usize) -> Option<[u32; N]> {
    let mut bytes = [[0; 4]; N];
    platform
        .read(address, bytes.as_flattened_mut())
        .then(|| bytes.map(u32::from_le_bytes))
}

fn reset_history(platform: &Platform, base: usize) -> Option<()> {
    let manager = platform.read_usize(base + TAA_MANAGER)?;
    let taa = taa::owner(platform, manager).filter(|&taa| taa != 0)?;
    taa::clear_main_history(platform, taa)
}

fn reconfigure(
    platform: &Platform,
    base: usize,
    output: [u32; 2],
    host: &rsf_game_host_services,
) -> Option<[u32; 2]> {
    if renderer_thread() != thread()
        || host.process_upscale.is_none()
        || host.render_config.is_none()
        || !output.into_iter().all(|value| (1..=16384).contains(&value))
    {
        return None;
    }
    if let Some(window) = platform
        .read_usize(base + ACTIVE_WINDOW)
        .filter(|&window| window != 0)
        && let Some(context) = platform
            .read_usize(base + IMMEDIATE_CONTEXT)
            .filter(|&context| context != 0)
        && let Some(chain) = window
            .checked_add(0x18)
            .and_then(|address| platform.read_usize(address))
            .filter(|&chain| chain != 0)
    {
        // The GameSDK record has only numeric, array and raw pointer fields.
        let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
        pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
        pass.role = RSF_GAME_RENDER_SR;
        pass.session_id = host.session_id;
        pass.flags = RSF_GAME_RENDER_RECONFIGURE | RSF_GAME_RENDER_NO_FG;
        pass.swapchain = ptr::with_exposed_provenance_mut(chain);
        [pass.camera.output_width, pass.camera.output_height] = output;
        let _context = unsafe {
            ComRef::<ID3D11DeviceContext>::retain(ptr::with_exposed_provenance_mut(context))
        }?;
        let _chain = unsafe { ComRef::<IDXGISwapChain>::retain(pass.swapchain.cast()) }?;
        unsafe { rsf_skyrim_call_upscale(host, ptr::with_exposed_provenance_mut(context), &pass) };
    }
    let mut config = rsf_game_render_config::request();
    unsafe { rsf_skyrim_call_render_config(host, &mut config) };
    config.checked_render_extent(output)
}

pub(crate) fn configure(
    platform: &Platform,
    pointer: usize,
    host: &rsf_game_host_services,
) -> Option<()> {
    let _transaction = MUTATION.try_lock().ok()?;
    let base = platform.main_module();
    if base == 0 || pointer != base + RENDER_STATE || state().output.is_some() {
        return None;
    }
    let output = words::<2>(platform, pointer + 0x24)?;
    let previous = state().owner;
    let Some(render) = reconfigure(platform, base, output, host) else {
        if let Some(previous) = previous {
            // After restoring the saved fields, the native resolver can resume ownership.
            unsafe {
                write_values(pointer + RATIO, &previous.saved_ratios);
                ptr::with_exposed_provenance_mut(pointer + RATIO_LOCK).write(previous.saved_lock);
            }
            state().owner = None;
            let _ = reset_history(platform, base);
            platform.log(
                host,
                c"Skyrim native resolution ownership released; original ratios restored.",
            );
        }
        return Some(());
    };
    let [frame] = words(platform, base + NATIVE_FRAME)?;
    let ratios = [
        render[0] as f32 / output[0] as f32,
        render[1] as f32 / output[1] as f32,
    ];
    let phases = ((8.0 * f64::from(output[0]) * f64::from(output[0])
        / (f64::from(render[0]) * f64::from(render[0]))) as u32)
        .clamp(8, 256);
    let sample = frame % phases + 1;
    let jitter = [
        -2.0 * (halton(sample, 2) - 0.5) / render[0] as f32,
        2.0 * (halton(sample, 3) - 0.5) / render[1] as f32,
    ];
    let (saved_ratios, saved_lock, prior) = if let Some(previous) = previous {
        (
            previous.saved_ratios,
            previous.saved_lock,
            previous.last_ratios,
        )
    } else {
        let saved = words::<4>(platform, pointer + RATIO)?.map(f32::from_bits);
        let [lock] = words(platform, pointer + RATIO_LOCK)?;
        (saved, lock, [saved[0], saved[1]])
    };
    let changed =
        previous.is_none_or(|previous| previous.render != render || previous.output != output);
    unsafe {
        write_values(pointer + RATIO, &[ratios[0], ratios[1], prior[0], prior[1]]);
        ptr::with_exposed_provenance_mut::<u32>(pointer + RATIO_LOCK).write(0);
        write_values(
            pointer + JITTER,
            &[jitter[0], jitter[1], jitter[0], jitter[1]],
        );
    }
    state().owner = Some(Owner {
        pointer,
        session: host.session_id,
        output,
        render,
        saved_ratios,
        saved_lock,
        last_ratios: ratios,
    });
    if changed {
        let _ = reset_history(platform, base);
        if host.log.is_some() {
            platform.log_text(host, &format!(
                "Skyrim native resolution plan {}x{} -> {}x{} ratios={},{} phases={} saved_lock={}; native auxiliary handoff required for reduced SR",
                render[0], render[1], output[0], output[1], ratios[0], ratios[1], phases, saved_lock,
            ));
        }
    }
    Some(())
}

unsafe fn write_values<const N: usize>(address: usize, values: &[f32; N]) {
    unsafe {
        ptr::copy_nonoverlapping(
            values.as_ptr(),
            ptr::with_exposed_provenance_mut(address),
            N,
        )
    };
}

fn upload(base: usize, extent: [u32; 2], flags: u8) {
    type Upload = unsafe extern "C" fn(*mut c_void, u8);
    type Viewport = unsafe extern "C" fn(*mut c_void, u32, u32, u8) -> usize;
    let renderer = ptr::with_exposed_provenance_mut(base + RENDERER);
    // This exact renderer and the two native entries were admitted by the observer guards.
    unsafe {
        ptr::with_exposed_provenance_mut::<u8>(base + UPLOAD_DIRTY).write(1);
        let upload: Upload = std::mem::transmute(base + UPLOAD);
        let viewport: Viewport = std::mem::transmute(base + VIEWPORT);
        upload(renderer, flags);
        viewport(renderer, extent[0], extent[1], 0);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_resolution_owned() -> bool {
    state().owner.is_some()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_resolution_controls(pointer: usize) -> bool {
    state().owner.is_some_and(|owner| owner.pointer == pointer)
}

pub(crate) fn extent(width: u32, height: u32) -> Option<[u32; 2]> {
    state()
        .owner
        .filter(|owner| owner.output == [width, height])
        .map(|owner| owner.render)
}

pub(crate) fn record_per_frame_flags(session: u64, frame: u32, flags: u8) {
    FRAME_FLAGS.set(Some(FrameFlags {
        session,
        frame,
        flags,
    }));
}

fn current_flags(platform: &Platform, owner: Owner, pass: &rsf_game_render_pass) -> Option<u8> {
    let observation = FRAME_FLAGS.get()?;
    let frame = platform.read_u32(owner.pointer - RENDER_STATE + NATIVE_FRAME)?;
    (frame == observation.frame
        && u64::from(frame) == pass.native_frame
        && owner.session == observation.session
        && owner.session == pass.session_id)
        .then_some(observation.flags)
}

pub(crate) fn output_ready(platform: &Platform, pass: &rsf_game_render_pass) -> bool {
    let state = state();
    state.output.is_none()
        && state
            .owner
            .is_some_and(|owner| current_flags(platform, owner, pass).is_some())
}

pub(crate) fn enter(platform: &Platform, pass: &rsf_game_render_pass) -> bool {
    let Ok(_transaction) = MUTATION.try_lock() else {
        return false;
    };
    let owner = {
        let state = state();
        if state.output.is_some() {
            return false;
        }
        let Some(owner) = state.owner else {
            return false;
        };
        owner
    };
    let Some(flags) = current_flags(platform, owner, pass) else {
        return false;
    };
    let thread = thread();
    let Some(numbers) = words::<4>(platform, owner.pointer + RATIO) else {
        return false;
    };
    unsafe { write_values(owner.pointer + RATIO, &[1.0; 4]) };
    upload(owner.pointer - RENDER_STATE, owner.output, flags);
    state().output = Some(Output {
        producer_ratios: numbers.map(f32::from_bits),
        thread,
        flags,
    });
    true
}

pub(crate) fn leave() -> bool {
    let Ok(_transaction) = MUTATION.try_lock() else {
        return false;
    };
    let (owner, output) = {
        let state = state();
        let Some(output) = state.output else {
            return true;
        };
        let Some(owner) = state.owner else {
            return false;
        };
        (owner, output)
    };
    if output.thread != thread() {
        return false;
    }
    unsafe { write_values(owner.pointer + RATIO, &output.producer_ratios) };
    upload(owner.pointer - RENDER_STATE, owner.render, output.flags);
    state().output = None;
    true
}
