// SPDX-License-Identifier: MIT
//! One native camera-cache reader for Skyrim's pre-HDR SR and FG producers.

use super::{Matrix, NativeCamera, decode};
use crate::platform::Platform;
use crate::taa;
use rsf_game_sys::camera_math::finite_matrix_close;
use rsf_game_sys::{RSF_GAME_RENDER_RESET, rsf_game_render_pass};
use std::sync::{Mutex, PoisonError};

const RENDER_STATE: usize = 0x333_63a0;
const ACTIVE_CAMERA: usize = 0x33d_3d28;
const NATIVE_FRAME: usize = 0x333_63f4;
const ACTIVE_WINDOW: usize = 0x333_0198;
const CACHE_STRIDE: usize = 0x290;

pub(crate) struct RenderCamera {
    pub(crate) camera: usize,
    pub(crate) window: usize,
    pub(crate) frame: u32,
    pub(crate) output: [u32; 2],
    pub(crate) render: [u32; 2],
    pub(crate) ratios: [f32; 2],
}

#[repr(C)]
pub(crate) struct Leaves {
    extent: unsafe extern "C" fn(u32, u32, *mut u32, *mut u32) -> bool,
    clock: unsafe extern "C" fn(*mut i64, *mut i64) -> bool,
}

const _: () = assert!(size_of::<Leaves>() == 16);

struct History {
    session: u64,
    camera: usize,
    frame: u32,
    render: [u32; 2],
    view_projection: Matrix,
    origin_bits: [u32; 3],
    jitter: [f32; 2],
    time: i64,
}

// SR and FG observe the same native frame at their own call sites. Each needs its own
// previous callback time and camera receipt. No native pointer is dereferenced from history.
static ACCEPTED_SR: Mutex<Option<(u64, u32)>> = Mutex::new(None);

pub(crate) fn accept_sr(session: u64, frame: u32) {
    *ACCEPTED_SR.lock().unwrap_or_else(PoisonError::into_inner) = Some((session, frame));
}

static HISTORY: Mutex<[Option<History>; 2]> = Mutex::new([None, None]);

fn read<const N: usize>(platform: &Platform, address: usize) -> Option<[u8; N]> {
    let mut bytes = [0; N];
    platform.read(address, &mut bytes).then_some(bytes)
}

fn read_floats<const N: usize>(platform: &Platform, address: usize) -> Option<[f32; N]> {
    let mut words = [[0; 4]; N];
    platform
        .read(address, words.as_flattened_mut())
        .then(|| words.map(f32::from_le_bytes))
}

fn camera_entry(platform: &Platform, cache: usize, count: u32, camera: usize) -> Option<usize> {
    let mut selected = None;
    for i in 0..count as usize {
        let entry = cache.checked_add(i * CACHE_STRIDE)?;
        let owner = platform.read_usize(entry)?;
        let [jittered] = read(platform, entry.checked_add(0x284)?)?;
        if owner == camera && jittered == 1 && selected.replace(entry).is_some() {
            return None;
        }
    }
    selected
}

fn read_cache(
    platform: &Platform,
    base: usize,
    manager: usize,
) -> Option<(RenderCamera, NativeCamera)> {
    let state = base + RENDER_STATE;
    let taa = taa::owner(platform, manager)?;
    if taa == 0 || read::<1>(platform, taa.checked_add(taa::ENABLED)?)? == [0] {
        return None;
    }
    let camera = platform.read_usize(base + ACTIVE_CAMERA)?;
    let cache = platform.read_usize(state + 0xb8)?;
    let count = platform.read_u32(state + 0xc8)?;
    let width = platform.read_u32(state + 0x24)?;
    let height = platform.read_u32(state + 0x28)?;
    if camera == 0
        || cache == 0
        || !(1..=256).contains(&count)
        || !(1..=16384).contains(&width)
        || !(1..=16384).contains(&height)
    {
        return None;
    }
    let ratios = read_floats(platform, state + 0x114)?;
    let frame = platform.read_u32(base + NATIVE_FRAME)?;
    let window = platform.read_usize(base + ACTIVE_WINDOW)?;
    if window == 0 {
        return None;
    }
    let entry = camera_entry(platform, cache, count, camera)?;
    let native = NativeCamera {
        view: read_floats(platform, entry.checked_add(0x40)?)?,
        projection: read_floats(platform, entry.checked_add(0x1c0)?)?,
        view_projection: read_floats(platform, entry.checked_add(0x140)?)?,
        previous_view_projection: read_floats(platform, entry.checked_add(0x180)?)?,
        origin: read_floats(platform, entry.checked_add(0x26c)?)?,
        previous_origin: read_floats(platform, entry.checked_add(0x278)?)?,
        jitter_ndc: read_floats(platform, state + 0x44)?,
        near_plane: f32::from_bits(platform.read_u32(camera.checked_add(0x160)?)?),
        far_plane: f32::from_bits(platform.read_u32(camera.checked_add(0x164)?)?),
    };
    Some((
        RenderCamera {
            camera,
            window,
            frame,
            output: [width, height],
            render: [width, height],
            ratios,
        },
        native,
    ))
}

pub(crate) fn read_frame(
    platform: &Platform,
    leaves: &Leaves,
    manager: usize,
    fg: bool,
    session: u64,
    pass: &mut rsf_game_render_pass,
) -> Option<RenderCamera> {
    let base = platform.main_module();
    if base == 0 {
        return None;
    }
    let (mut facts, native) = read_cache(platform, base, manager)?;
    let [width, height] = facts.output;
    let [rw, rh] = &mut facts.render;
    let controlled = unsafe { (leaves.extent)(width, height, rw, rh) };
    if (!controlled && facts.ratios != [1.0; 2])
        || !facts.ratios.iter().zip(facts.render).zip(facts.output).all(
            |((&ratio, render), output)| {
                ratio.is_finite() && (ratio - render as f32 / output as f32).abs() <= 0.00001
            },
        )
    {
        return None;
    }
    let [rw, rh] = facts.render;
    let (mut camera, history) = decode(&native, rw, rh)?;
    camera.output_width = width;
    camera.output_height = height;
    let mut now = 0;
    let mut frequency = 0;
    if !unsafe { (leaves.clock)(&mut now, &mut frequency) } || frequency <= 0 {
        return None;
    }
    let receipt = History {
        session,
        camera: facts.camera,
        frame: facts.frame,
        render: facts.render,
        view_projection: native.view_projection,
        origin_bits: native.origin.map(f32::to_bits),
        jitter: camera.jitter_pixels,
        time: now,
    };
    let previous =
        HISTORY.lock().unwrap_or_else(PoisonError::into_inner)[usize::from(fg)].replace(receipt)?;
    if previous.session != session {
        return None;
    }
    let elapsed = now.checked_sub(previous.time)? as f64 / frequency as f64;
    if previous.time == 0 || elapsed <= 0.0 || elapsed > 1.0 {
        return None;
    }
    camera.frame_time_seconds = elapsed as f32;
    let consumed = fg
        || *ACCEPTED_SR.lock().unwrap_or_else(PoisonError::into_inner)
            == Some((session, previous.frame));
    if consumed
        && history.is_some()
        && previous.camera == facts.camera
        && facts.frame == previous.frame.wrapping_add(1)
        && previous.render == facts.render
        && finite_matrix_close(
            &native.previous_view_projection,
            &previous.view_projection,
            0.001,
        )
        && native.previous_origin.map(f32::to_bits) == previous.origin_bits
    {
        camera.previous_jitter_pixels = previous.jitter;
    } else {
        pass.flags |= RSF_GAME_RENDER_RESET;
    }
    pass.camera = camera;
    pass.previous_clip_to_clip = history.unwrap_or(super::IDENTITY);
    Some(facts)
}
