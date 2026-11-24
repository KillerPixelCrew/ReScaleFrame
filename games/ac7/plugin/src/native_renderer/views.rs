// SPDX-License-Identifier: MIT
//! Renderer view preparation: the owned view rectangles at construction and the temporal
//! jitter selection around visibility.
use super::frame::bind_renderer;
use super::{Guard, engine, hook, installed, key, original, primary_view, read, scene_targets};
use rsf_ac7::discovery::Site;
use rsf_game_sys::rsf_game_render_config;
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::write_unaligned;
use std::sync::atomic::{AtomicU32, Ordering};

const VIEW_STRIDE: u64 = 0x27c0;

/// Calls `visit(view address)` for each view of a renderer, stopping when it returns false.
/// False when the view array cannot be read or has an implausible count.
pub(super) fn for_each_view(renderer: *mut c_void, mut visit: impl FnMut(u64) -> bool) -> bool {
    let (mut storage, mut count) = (0u64, 0i32);
    if !read(key(renderer), 0xb8, &mut storage)
        || !read(key(renderer), 0xc0, &mut count)
        || !(1..=16).contains(&count)
    {
        return false;
    }
    for index in 0..count as u64 {
        if !visit(storage.wrapping_add(index * VIEW_STRIDE)) {
            break;
        }
    }
    true
}

fn write_rect(view: u64, offset: u64, rect: [i32; 4]) {
    unsafe { write_unaligned((view + offset) as usize as *mut [i32; 4], rect) };
}

fn write_mode(view: u64, mode: u32) {
    unsafe { write_unaligned((view + 0x13c0) as usize as *mut u32, mode) };
}

pub(super) fn prepare_owned_views(renderer: *mut c_void, config: &rsf_game_render_config) {
    let mut changed = false;
    let width = if config.enabled != 0 {
        config.render_width
    } else {
        config.output_width
    };
    let height = if config.enabled != 0 {
        config.render_height
    } else {
        config.output_height
    };
    if width == 0
        || height == 0
        || width > config.output_width
        || height > config.output_height
        || config.output_width > 16384
        || config.output_height > 16384
    {
        return;
    }
    for_each_view(renderer, |view| {
        let (mut mode, mut state, mut rect) = (0u32, 0u64, [0i32; 4]);
        if !read(view, 0x80, &mut rect)
            || !read(view, 0x13c0, &mut mode)
            || !read(view, 8, &mut state)
            || mode != 1
            || state == 0
            || !primary_view(view, config)
            || rect[0] < 0
            || rect[1] < 0
            || rect[2] <= rect[0]
            || rect[3] <= rect[1]
        {
            return true;
        }
        // Keep the active rectangle at the backend's exact size. The engine pads allocations
        // separately; Ultra Performance can accept a single width that is not divisible by four.
        let scale = |value: i32, input: u32, output: u32| {
            ((value as u64)
                .wrapping_mul(u64::from(input))
                .wrapping_add(u64::from(output / 2))
                / u64::from(output)) as i32
        };
        let x = scale(rect[0], width, config.output_width);
        let y = scale(rect[1], height, config.output_height);
        let scaled = [
            x,
            y,
            x.wrapping_add(scale(rect[2] - rect[0], width, config.output_width)),
            y.wrapping_add(scale(rect[3] - rect[1], height, config.output_height)),
        ];
        write_rect(view, 0x70, scaled);
        changed = true;
        true
    });
    if changed {
        unsafe {
            engine::<unsafe extern "C" fn(*mut c_void)>(Site::FamilySize)(
                renderer.cast::<u8>().wrapping_add(0x10).cast(),
            )
        };
    }
}

pub(super) unsafe extern "C" fn hooked_construct(
    renderer: *mut c_void,
    family: *mut c_void,
    hit_proxy: *mut c_void,
) -> *mut c_void {
    let _guard = Guard::entry();
    let construct: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
        unsafe { original(hook::CONSTRUCT) };
    let result = unsafe { construct(renderer, family, hit_proxy) };
    if let Some(owner) = installed()
        && owner.active()
    {
        bind_renderer(owner, renderer);
        if let Some(config) = owner.config() {
            prepare_owned_views(renderer, &config);
        }
    }
    result
}

#[derive(Clone, Copy)]
struct TemporalSampleOwner {
    state: u64,
    frame: u32,
    phases: u32,
}

const NO_SAMPLE_OWNER: TemporalSampleOwner = TemporalSampleOwner {
    state: 0,
    frame: 0,
    phases: 8,
};

thread_local! {
    static TEMPORAL_SAMPLE_OWNERS: Cell<[TemporalSampleOwner; 16]> =
        const { Cell::new([NO_SAMPLE_OWNER; 16]) };
}

pub(super) unsafe extern "C" fn hooked_temporal_sample_index(state: *mut c_void) -> u32 {
    let _entry = Guard::entry();
    for owner in TEMPORAL_SAMPLE_OWNERS.get() {
        if owner.state != 0 && owner.state == key(state) {
            let index = (owner.frame % owner.phases) as u8;
            let phases = owner.phases as u8;
            // Preserve the native jitter producer and history matrices, replacing its fixed
            // eight-sample setting at the view-state boundary where it consumes the phase.
            unsafe {
                *state.cast::<u8>().add(0xc1c) = index;
                *state.cast::<u8>().add(0xc1d) = phases;
            }
            return u32::from(index);
        }
    }
    let forward: unsafe extern "C" fn(*mut c_void) -> u32 =
        unsafe { original(hook::TEMPORAL_SAMPLE_INDEX) };
    unsafe { forward(state) }
}

static LAST_PHASES: AtomicU32 = AtomicU32::new(0);

/// The rounded phase count clamped to 8..=255. A NaN from an empty rectangle keeps the stock
/// eight phases.
fn sample_phases(value: f64) -> u32 {
    let rounded = value.round();
    if rounded.is_nan() {
        8
    } else {
        rounded.clamp(8.0, 255.0) as u32
    }
}

pub(super) unsafe extern "C" fn hooked_visibility(
    renderer: *mut c_void,
    list: *mut c_void,
    third: usize,
    fourth: *mut c_void,
) {
    let _guard = Guard::entry();
    let owner = installed();
    let mut views = [0u64; 16];
    let mut selected = 0usize;
    let previous_samples = TEMPORAL_SAMPLE_OWNERS.replace([NO_SAMPLE_OWNER; 16]);
    let mut buffer = [0i32; 2];
    read(key(scene_targets()), 0x208, &mut buffer);
    if let Some(owner) = owner
        && owner.active()
        && let Some(config) = owner.config()
        && config.enabled != 0
    {
        for_each_view(renderer, |view| {
            let (mut rect, mut mode, mut state) = ([0i32; 4], 0u32, 0u64);
            if !(read(view, 0x80, &mut rect)
                && read(view, 0x13c0, &mut mode)
                && read(view, 8, &mut state)
                && state != 0
                && mode == 1
                && primary_view(view, &config)
                && rect[0] >= 0
                && rect[1] >= 0
                && rect[2] > rect[0]
                && rect[3] > rect[1])
            {
                return true;
            }
            let mut render = [0i32; 4];
            read(view, 0x70, &mut render);
            let cropped = rect[0] != 0
                || rect[1] != 0
                || rect[2] != config.output_width as i32
                || rect[3] != config.output_height as i32;
            let expected_width = config.render_width.wrapping_add(3) & !3;
            let expected_height = config.render_height.wrapping_add(3) & !3;
            let allocated = buffer[0] == expected_width as i32
                && buffer[1] == expected_height as i32
                && (cropped
                    || (render[2].wrapping_sub(render[0]) == config.render_width as i32
                        && render[3].wrapping_sub(render[1]) == config.render_height as i32));
            if !allocated || render[2] > buffer[0] || render[3] > buffer[1] {
                // Engine buffer hysteresis can retain a larger allocation after a scale
                // change. Render a complete unjittered native frame until allocation and
                // viewport agree; never feed padded bloom/depth regions to the SR graph.
                if buffer[0] >= rect[2] && buffer[1] >= rect[3] {
                    write_rect(view, 0x70, rect);
                }
                return true;
            }
            views[selected] = view;
            selected += 1;
            let (mut temporal_state, mut family, mut frame) = (0u64, 0u64, 0u32);
            if read(view, 0x1410, &mut temporal_state)
                && temporal_state != 0
                && read(view, 0, &mut family)
                && read(family, 0x68, &mut frame)
            {
                let phases = sample_phases(
                    8.0 * f64::from(config.output_width) * f64::from(config.output_height)
                        / (f64::from(render[2].wrapping_sub(render[0]))
                            * f64::from(render[3].wrapping_sub(render[1]))),
                );
                let mut owners = TEMPORAL_SAMPLE_OWNERS.get();
                owners[selected - 1] = TemporalSampleOwner {
                    state: temporal_state,
                    frame,
                    phases,
                };
                TEMPORAL_SAMPLE_OWNERS.set(owners);
                if LAST_PHASES.swap(phases, Ordering::Relaxed) != phases {
                    owner.log_bounded(
                        &format!(
                            "native temporal sampling: {} phases for {}x{} from {}x{}",
                            phases,
                            config.output_width,
                            config.output_height,
                            render[2].wrapping_sub(render[0]),
                            render[3].wrapping_sub(render[1])
                        ),
                        112,
                    );
                }
            }
            write_mode(view, 2);
            true
        });
    }
    // Select the stock temporal preparation only here. DOF/SSR and native AA graph selection see
    // their original modes afterward. Engine histories and derived projection values stay native.
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void, usize, *mut c_void) =
        unsafe { original(hook::VISIBILITY) };
    unsafe { forward(renderer, list, third, fourth) };
    // Every selected view was in mode 1.
    for &view in &views[..selected] {
        write_mode(view, 1);
    }
    TEMPORAL_SAMPLE_OWNERS.set(previous_samples);
}
