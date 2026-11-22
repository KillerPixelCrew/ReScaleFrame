// SPDX-License-Identifier: MIT
//! Decode the verified UE4.27 view and family fields under the native guarded copy.
use rsf_game_sys::camera_math::{inverse_4x4, multiply_4x4 as multiply};
use rsf_game_sys::{RSF_GAME_CAMERA_ABI_VERSION, rsf_game_render_pass};
use std::ffi::c_void;
use std::mem::MaybeUninit;

pub(crate) type CopyMemory = unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> bool;
type Matrix = [f32; 16];

const FAMILY: usize = 0;
const FAMILY_FRAME_TIME: usize = 0x54;
const FAMILY_FRAME: usize = 0x5c;
const PROJECTION: usize = 0x2c0;
const WORLD_TO_VIEW: usize = 0x340;
const VIEW_TO_WORLD: usize = 0x380;
const CURRENT_TRANSLATED: usize = 0x480;
const CURRENT_TRANSLATION: usize = 0x600;
pub(crate) const JITTER: usize = 0x4fa4;
const PREVIOUS_PROJECTION: usize = 0x5010;
const PREVIOUS_TRANSLATED: usize = 0x51d0;
const PREVIOUS_TRANSLATION: usize = 0x5350;

// SAFETY: every bit pattern of T must be valid. The caller uses native numeric fields and arrays.
pub(crate) unsafe fn read<T: Copy>(copy: CopyMemory, base: usize, offset: usize) -> Option<T> {
    let address = base.checked_add(offset).filter(|_| base != 0)?;
    let mut value = MaybeUninit::<T>::uninit();
    if unsafe {
        copy(
            value.as_mut_ptr().cast(),
            address as *const c_void,
            size_of::<T>(),
        )
    } {
        Some(unsafe { value.assume_init() })
    } else {
        None
    }
}

fn inverse(input: Matrix) -> Option<Matrix> {
    if !input.iter().all(|value| value.is_finite()) {
        return None;
    }
    Some(inverse_4x4(&input.map(f64::from))?.map(|value| value as f32))
}

fn extent(rect: [i32; 4]) -> Option<[u32; 2]> {
    let width = rect[2].checked_sub(rect[0]).filter(|&width| width > 0)?;
    let height = rect[3].checked_sub(rect[1]).filter(|&height| height > 0)?;
    Some([width as u32, height as u32])
}

fn decode(
    view: usize,
    pass: &mut rsf_game_render_pass,
    copy: CopyMemory,
    jitter: Option<[f32; 2]>,
) -> Option<()> {
    let camera = &mut pass.camera;
    // These exact view and family offsets are verified for the matched Wingman build. The native
    // leaf copies each field so an unreadable view is a refusal rather than a Rust reference.
    let family: usize = unsafe { read(copy, view, FAMILY) }?;
    let frame: u32 = unsafe { read(copy, family, FAMILY_FRAME) }?;
    camera.frame_time_seconds = unsafe { read(copy, family, FAMILY_FRAME_TIME) }?;
    camera.view_to_clip = unsafe { read(copy, view, PROJECTION) }?;
    camera.world_to_view = unsafe { read(copy, view, WORLD_TO_VIEW) }?;
    camera.view_to_world = unsafe { read(copy, view, VIEW_TO_WORLD) }?;
    let current_translated: Matrix = unsafe { read(copy, view, CURRENT_TRANSLATED) }?;
    let current_translation: [f32; 3] = unsafe { read(copy, view, CURRENT_TRANSLATION) }?;
    let previous_projection: Matrix = unsafe { read(copy, view, PREVIOUS_PROJECTION) }?;
    let previous_translated: Matrix = unsafe { read(copy, view, PREVIOUS_TRANSLATED) }?;
    let previous_translation: [f32; 3] = unsafe { read(copy, view, PREVIOUS_TRANSLATION) }?;
    let jitter = jitter?;

    let frame_time = camera.frame_time_seconds;
    let projection = camera.view_to_clip;
    if frame == u32::MAX
        || !frame_time.is_finite()
        || frame_time <= 0.0
        || frame_time > 1.0
        || jitter.iter().any(|value| !value.is_finite())
        || projection[15].abs() > 1e-6
        || projection[0] <= 0.0
        || projection[5] <= 0.0
        || camera
            .view_to_world
            .iter()
            .chain(&camera.world_to_view)
            .chain(&previous_projection)
            .any(|value| !value.is_finite())
    {
        return None;
    }
    camera.clip_to_view = inverse(projection)?;
    let identity = multiply(&camera.world_to_view, &camera.view_to_world);
    if identity.iter().enumerate().any(|(index, value)| {
        let expected = if index % 5 == 0 { 1.0 } else { 0.0 };
        (*value - expected).abs() > 0.02
    }) {
        return None;
    }

    let transposed = std::array::from_fn(|index| current_translated[(index % 4) * 4 + index / 4]);
    let mut delta = [0.0; 16];
    for index in [0, 5, 10, 15] {
        delta[index] = 1.0;
    }
    for index in 0..3 {
        delta[12 + index] = previous_translation[index] - current_translation[index];
    }
    let inverse_current = multiply(&camera.clip_to_view, &transposed);
    let previous = multiply(&delta, &previous_translated);
    let previous = multiply(&previous, &previous_projection);
    camera.clip_to_previous_clip = multiply(&inverse_current, &previous);
    pass.previous_clip_to_clip = inverse(camera.clip_to_previous_clip)?;

    [camera.render_width, camera.render_height] = extent(pass.render_rect)?;
    [camera.output_width, camera.output_height] = extent(pass.output_rect)?;
    camera.struct_size = size_of_val(camera) as u32;
    camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    camera.near_plane = projection[14] / (1.0 - projection[10]);
    camera.far_plane = if projection[10] < 0.0 {
        -projection[14] / projection[10]
    } else {
        f32::MAX
    };
    camera.vertical_fov_radians = 2.0 * (1.0 / projection[5]).atan();
    camera.depth_inverted = 1;
    camera.motion_jittered = 0;
    pass.family_key = family as u64;
    pass.native_frame = u64::from(frame);
    pass.motion_to_uv = [0.5, -0.5];
    pass.motion_camera_included = 1;
    (camera.near_plane.is_finite()
        && camera.near_plane > 0.0
        && camera.far_plane > camera.near_plane)
        .then_some(())
}

pub(crate) fn read_jitter(view: usize, copy: CopyMemory) -> Option<[f32; 2]> {
    unsafe { read(copy, view, JITTER) }
}

pub(crate) fn read_camera(
    view: usize,
    pass: &mut rsf_game_render_pass,
    copy: CopyMemory,
    jitter: Option<[f32; 2]>,
) -> bool {
    pass.camera_valid = 0;
    pass.camera.jitter_pixels = jitter.unwrap_or_default();
    let valid = decode(view, pass, copy, jitter).is_some();
    pass.camera_valid = u32::from(valid);
    valid
}

#[cfg(all(windows, test))]
#[path = "camera_fixture.rs"]
mod fixture;
