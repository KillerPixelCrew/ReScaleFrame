// SPDX-License-Identifier: MIT
//! Owns the AC7 capture-derived scene colour watch and its retained recombine layers.
#[cfg(windows)]
use crate::com_resource::{release, retain};
#[cfg(windows)]
use crate::scene_color_abi::{RSF_FRAME_TAP_MAX_INPUTS, rsf_frame_tap_target_draw};
use crate::scene_color_abi::{rsf_ac7_scene_color, rsf_frame_tap_geometry};
#[cfg(windows)]
use crate::ui_abi::{RSF_AC7_FORMAT_B8G8R8A8_UNORM, RSF_AC7_FORMAT_B8G8R8A8_UNORM_SRGB};
use std::ffi::{c_int, c_void};
#[cfg(windows)]
use std::{ptr, slice};

#[cfg(windows)]
const R16G16B16A16_FLOAT: u32 = 10;
#[cfg(windows)]
const R11G11B10_FLOAT: u32 = 26;
#[cfg(windows)]
const R8G8B8A8_UNORM: u32 = 28;
#[cfg(windows)]
const R8G8B8A8_UNORM_SRGB: u32 = 29;

#[cfg(windows)]
fn layer_size_fits(width: u32, height: u32, scene_width: u32, scene_height: u32) -> bool {
    if width == 0 || height == 0 || scene_width == 0 || scene_height == 0 {
        return false;
    }
    if width < scene_width.div_ceil(2)
        || u64::from(width) > u64::from(scene_width) * 4
        || height < scene_height.div_ceil(2)
        || u64::from(height) > u64::from(scene_height) * 4 + 4
    {
        return false;
    }
    let a = u64::from(width) * u64::from(scene_height);
    let b = u64::from(height) * u64::from(scene_width);
    a.abs_diff(b) <= b / 50
}

#[cfg(windows)]
fn release_layers(state: &mut rsf_ac7_scene_color) {
    let count = (state.composed_layer_count as usize).min(state.composed_layers.len());
    for resource in &mut state.composed_layers[..count] {
        unsafe { release(std::mem::replace(resource, ptr::null_mut())) };
    }
    state.composed_layer_count = 0;
}

#[cfg(windows)]
fn clear(state: &mut rsf_ac7_scene_color) {
    unsafe {
        release(state.source);
        release(state.composed);
    }
    release_layers(state);
    *state = rsf_ac7_scene_color::default();
}

// Each native call exclusively lends the render-thread state and its retained resources.
#[cfg(windows)]
#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_scene_color_clear(state: &mut rsf_ac7_scene_color) {
    clear(state);
}

#[cfg(windows)]
#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_scene_color_source(
    state: &mut rsf_ac7_scene_color,
    source: *mut c_void,
    context: *mut c_void,
    width: u32,
    height: u32,
) -> c_int {
    if state.source == source
        && state.context == context
        && state.width == width
        && state.height == height
    {
        return 0;
    }
    // The old source may be the same allocation with a new context or extent.
    unsafe { retain(source) };
    clear(state);
    state.source = source;
    state.context = context;
    state.width = width;
    state.height = height;
    1
}

/// The tap lends a full draw and its borrowed inputs for this call.
#[cfg(windows)]
unsafe fn target_draw<'a>(
    draw: *const rsf_frame_tap_target_draw,
) -> Option<&'a rsf_frame_tap_target_draw> {
    if draw.is_null()
        || (unsafe { draw.cast::<u32>().read() } as usize) < size_of::<rsf_frame_tap_target_draw>()
    {
        return None;
    }
    unsafe { draw.as_ref() }
}

#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_scene_color_draw(
    state: &mut rsf_ac7_scene_color,
    draw: *const rsf_frame_tap_target_draw,
) {
    let Some(draw) = (unsafe { target_draw(draw) }) else {
        return;
    };
    // Follow one linear recombine from the identified source, before the byte-colour boundary.
    if state.source.is_null()
        || state.composed_this_frame != 0
        || state.search_closed != 0
        || draw.context != state.context
        || draw.render_target.is_null()
        || draw.render_target == state.source
        || draw.depth_bound != 0
        || draw.target_count != 1
        || draw.target_samples != 1
        || draw.inputs_truncated != 0
        || draw.element_count != 3
        || draw.target_width != state.width
        || draw.target_height != state.height
        || draw.viewport_width != state.width
        || draw.viewport_height != state.height
        || draw.viewport_x != 0.0
        || draw.viewport_y != 0.0
        || draw.inputs.is_null()
        || draw.input_count > RSF_FRAME_TAP_MAX_INPUTS
    {
        return;
    }

    let inputs = unsafe { slice::from_raw_parts(draw.inputs, draw.input_count as usize) };
    let mut reads_source = false;
    let mut layers = [ptr::null_mut(); 8];
    let mut count = 0;
    for input in inputs {
        if input.texture == state.source
            && input.format == R11G11B10_FLOAT
            && input.width == state.width
            && input.height == state.height
        {
            reads_source = true;
        }
        if !input.texture.is_null()
            && input.texture != state.source
            && input.format == R16G16B16A16_FLOAT
            && layer_size_fits(input.width, input.height, state.width, state.height)
            && count < layers.len()
            && !layers[..count].contains(&input.texture)
        {
            layers[count] = input.texture;
            count += 1;
        }
    }

    if !reads_source {
        return;
    }
    if matches!(
        draw.target_format,
        RSF_AC7_FORMAT_B8G8R8A8_UNORM
            | RSF_AC7_FORMAT_B8G8R8A8_UNORM_SRGB
            | R8G8B8A8_UNORM
            | R8G8B8A8_UNORM_SRGB
    ) {
        state.search_closed = 1;
        return;
    }
    if count == 0 || draw.target_format != R11G11B10_FLOAT {
        return;
    }

    if state.composed != draw.render_target {
        unsafe {
            retain(draw.render_target);
            release(state.composed);
        }
        state.composed = draw.render_target;
    }
    for &layer in &layers[..count] {
        unsafe { retain(layer) };
    }
    release_layers(state);
    state.composed_layers = layers;
    state.composed_layer_count = count as u32;
    state.composed_this_frame = 1;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_scene_color_selected(
    state: &rsf_ac7_scene_color,
    source: *mut c_void,
) -> *mut c_void {
    if state.source == source && state.composed_this_frame != 0 {
        state.composed
    } else {
        source
    }
}

#[cfg(windows)]
#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_scene_color_end_frame(state: &mut rsf_ac7_scene_color) {
    release_layers(state);
    state.composed_this_frame = 0;
    state.search_closed = 0;
}

/// The tap lends the geometry facts for this draw or null.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_scene_depth_candidate(draw: *const rsf_frame_tap_geometry) -> c_int {
    c_int::from(unsafe { draw.as_ref() }.is_some_and(|draw| {
        !draw.target.is_null()
            && !draw.depth_view.is_null()
            && draw.samples == 1
            && draw.format == R16G16B16A16_FLOAT
    }))
}

/// The tap lends the same target draw facts before the first draw or null.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_separate_translucency_draw(
    draw: *const rsf_frame_tap_target_draw,
) -> c_int {
    c_int::from(unsafe { draw.as_ref() }.is_some_and(|draw| {
        !draw.render_target.is_null()
            && draw.depth_bound != 0
            && draw.target_count == 1
            && draw.target_samples == 1
            && draw.target_format == R16G16B16A16_FLOAT
    }))
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_ac7_translucency_scale(
    render_percent: u32,
    target_percent: u32,
) -> f32 {
    if target_percent == 0 {
        return 1.0;
    }
    let render = if render_percent == 0 {
        100
    } else {
        render_percent
    };
    target_percent as f32 / render as f32
}
