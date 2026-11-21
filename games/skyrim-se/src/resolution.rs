// SPDX-License-Identifier: MIT
//! Owns Skyrim's engine render ratios, jitter and temporary output viewport.

use crate::platform::{self, Platform};
use rsf_game_sys::{
    RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_RECONFIGURE, RSF_GAME_RENDER_SR, rsf_game_host_services,
    rsf_game_render_config, rsf_game_render_pass,
};
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Mutex, MutexGuard, PoisonError};

const RENDER_STATE: usize = 0x333_63a0;
const RENDERER: usize = 0x333_1f40;
const RATIO: usize = 0x114;
const RATIO_LOCK: usize = 0x128;
const JITTER: usize = 0x44;
const NATIVE_FRAME: usize = 0x333_63f4;
const UPLOAD_DIRTY: usize = 0x333_6400;
const UPLOAD: usize = 0x100_ae80;
const VIEWPORT: usize = 0x100_9960;
const TAA_MANAGER: usize = 0x33d_40a0;
const ACTIVE_WINDOW: usize = 0x333_0198;
const IMMEDIATE_CONTEXT: usize = 0x333_1f30;

#[repr(C)]
struct Report {
    render: [u32; 2],
    output: [u32; 2],
    ratios: [f32; 2],
    phases: u32,
    saved_lock: u32,
}

#[repr(C)]
struct Upload {
    renderer: usize,
    dirty: usize,
    upload: usize,
    viewport: usize,
    extent: [u32; 2],
    flags: u8,
}

#[repr(C)]
struct Leaves {
    renderer_thread: unsafe extern "C" fn() -> bool,
    thread: unsafe extern "C" fn() -> u32,
    prepare: unsafe extern "C" fn(&rsf_game_host_services, *mut c_void, &rsf_game_render_pass),
    config: unsafe extern "C" fn(&rsf_game_host_services, &mut rsf_game_render_config),
    ratios: unsafe extern "C" fn(usize, &[f32; 4]),
    lock: unsafe extern "C" fn(usize, u32),
    jitter: unsafe extern "C" fn(usize, &[f32; 2]),
    upload: unsafe extern "C" fn(&Upload),
    report: unsafe extern "C" fn(&rsf_game_host_services, &Report),
}

const _: () = {
    assert!(size_of::<Report>() == 32);
    assert!(size_of::<Upload>() == 48 && std::mem::offset_of!(Upload, flags) == 40);
    assert!(size_of::<Leaves>() == 72);
};

#[derive(Clone, Copy)]
struct Owner {
    pointer: usize,
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

thread_local! {
    static FRAME_FLAGS: Cell<Option<u8>> = const { Cell::new(None) };
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

fn reset_history(platform: &Platform, base: usize, leaves: &Leaves) -> Option<()> {
    let manager = platform.read_usize(base + TAA_MANAGER)?;
    if manager == 0 {
        return None;
    }
    let taa = platform.read_usize(manager.checked_add(0x1f0)?)?;
    if taa == 0 {
        return None;
    }
    let address = taa.checked_add(0x30)?;
    let [mask] = words(platform, address)?;
    unsafe { (leaves.lock)(address, mask | 1) };
    Some(())
}

fn halton(mut index: u32, base: u32) -> f32 {
    let mut value = 0.0;
    let mut factor = 1.0;
    while index != 0 {
        factor /= base as f32;
        value += factor * (index % base) as f32;
        index /= base;
    }
    value
}

fn reconfigure(
    platform: &Platform,
    base: usize,
    output: [u32; 2],
    host: &rsf_game_host_services,
    leaves: &Leaves,
) -> Option<rsf_game_render_config> {
    if !unsafe { (leaves.renderer_thread)() }
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
        unsafe { (leaves.prepare)(host, ptr::with_exposed_provenance_mut(context), &pass) };
    }
    let mut config = rsf_game_render_config {
        struct_size: size_of::<rsf_game_render_config>() as u32,
        enabled: 0,
        output_width: 0,
        output_height: 0,
        render_width: 0,
        render_height: 0,
    };
    unsafe { (leaves.config)(host, &mut config) };
    let render = [config.render_width, config.render_height];
    (config.enabled != 0
        && [config.output_width, config.output_height] == output
        && render
            .into_iter()
            .zip(output)
            .all(|(render, output)| (1..=output).contains(&render)))
    .then_some(config)
}

fn configure(
    platform: &Platform,
    pointer: usize,
    host: &rsf_game_host_services,
    leaves: &Leaves,
) -> Option<()> {
    let _transaction = MUTATION.try_lock().ok()?;
    let base = platform.main_module();
    if base == 0 || pointer != base + RENDER_STATE || state().output.is_some() {
        return None;
    }
    let output = words::<2>(platform, pointer + 0x24)?;
    let previous = state().owner;
    let Some(config) = reconfigure(platform, base, output, host, leaves) else {
        if let Some(previous) = previous {
            // After restoring the saved fields, the native resolver can resume ownership.
            unsafe {
                (leaves.ratios)(pointer + RATIO, &previous.saved_ratios);
                (leaves.lock)(pointer + RATIO_LOCK, previous.saved_lock);
            }
            state().owner = None;
            let _ = reset_history(platform, base, leaves);
            platform.log(
                host,
                c"Skyrim native resolution ownership released; original ratios restored.",
            );
        }
        return Some(());
    };
    let [frame] = words(platform, base + NATIVE_FRAME)?;
    let render = [config.render_width, config.render_height];
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
        (leaves.ratios)(pointer + RATIO, &[ratios[0], ratios[1], prior[0], prior[1]]);
        (leaves.lock)(pointer + RATIO_LOCK, 0);
        (leaves.jitter)(pointer + JITTER, &jitter);
    }
    state().owner = Some(Owner {
        pointer,
        output,
        render,
        saved_ratios,
        saved_lock,
        last_ratios: ratios,
    });
    if changed {
        let _ = reset_history(platform, base, leaves);
        if host.log.is_some() {
            unsafe {
                (leaves.report)(
                    host,
                    &Report {
                        render,
                        output,
                        ratios,
                        phases,
                        saved_lock,
                    },
                )
            };
        }
    }
    Some(())
}

fn upload(base: usize, leaves: &Leaves, extent: [u32; 2], flags: u8) {
    unsafe {
        (leaves.upload)(&Upload {
            renderer: base + RENDERER,
            dirty: base + UPLOAD_DIRTY,
            upload: base + UPLOAD,
            viewport: base + VIEWPORT,
            extent,
            flags,
        })
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_resolution_configure(
    platform: *const Platform,
    leaves: &Leaves,
    pointer: usize,
    host: &rsf_game_host_services,
) {
    if let Some(platform) = unsafe { platform::inspect(platform) } {
        let _ = configure(platform, pointer, host, leaves);
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

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_resolution_extent(
    width: u32,
    height: u32,
    rw: &mut u32,
    rh: &mut u32,
) -> bool {
    let Some(owner) = state()
        .owner
        .filter(|owner| owner.output == [width, height])
    else {
        return false;
    };
    [*rw, *rh] = owner.render;
    true
}

pub(crate) fn record_per_frame_flags(flags: u8) {
    FRAME_FLAGS.set(Some(flags));
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_resolution_output_ready() -> bool {
    let state = state();
    state.owner.is_some() && state.output.is_none() && FRAME_FLAGS.get().is_some()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_resolution_enter(
    platform: *const Platform,
    leaves: &Leaves,
) -> bool {
    let Ok(_transaction) = MUTATION.try_lock() else {
        return false;
    };
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
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
    let Some(flags) = FRAME_FLAGS.get() else {
        return false;
    };
    let thread = unsafe { (leaves.thread)() };
    let Some(numbers) = words::<4>(platform, owner.pointer + RATIO) else {
        return false;
    };
    unsafe { (leaves.ratios)(owner.pointer + RATIO, &[1.0; 4]) };
    upload(owner.pointer - RENDER_STATE, leaves, owner.output, flags);
    state().output = Some(Output {
        producer_ratios: numbers.map(f32::from_bits),
        thread,
    });
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_resolution_leave(leaves: &Leaves) -> bool {
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
    if output.thread != unsafe { (leaves.thread)() } {
        return false;
    }
    let Some(flags) = FRAME_FLAGS.get() else {
        return false;
    };
    unsafe { (leaves.ratios)(owner.pointer + RATIO, &output.producer_ratios) };
    upload(owner.pointer - RENDER_STATE, leaves, owner.render, flags);
    state().output = None;
    true
}
