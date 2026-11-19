// SPDX-License-Identifier: MIT
use crate::d3d11_output::{RestoreBindings, SaveBindings, SavedBindings, texture_desc};
use crate::d3d11_output_abi::RSF_D3D11_FORMAT_RGBA16_FLOAT;
use crate::fg_scene::with_scene;
use crate::native_sr_abi::{
    RSF_DLSS_PIPELINE_ABI_VERSION, rsf_dlss_pipeline_frame as Frame,
    rsf_native_translucency_masks as Masks,
};
use crate::native_sr_state::{camera_for, rsf_core_native_sr_commit, rsf_core_native_sr_reset};
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_RENDER_SR, rsf_camera_frame, rsf_game_render_pass as Pass,
};
use std::ffi::c_void;

const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const RESOURCE_FAILED: i32 = -7;
const SR: u32 = 0;
type Texture = *mut c_void;

#[repr(C)]
struct Services {
    validate_device: unsafe extern "C" fn(*mut c_void, *const Pass) -> i32,
    depth_layout: unsafe extern "C" fn(*const Pass) -> i32,
    save: SaveBindings,
    restore: RestoreBindings,
    clear_predication: unsafe extern "C" fn(*mut c_void),
    resize_output: unsafe extern "C" fn(u32, u32) -> i32,
    prepare_regions: unsafe extern "C" fn(*mut c_void, *const Pass, *mut Texture) -> i32,
    exposure: unsafe extern "C" fn(*mut c_void, Texture) -> Texture,
    take_translucency: unsafe extern "C" fn(*const Pass, *mut Masks) -> i32,
    merge_mask: unsafe extern "C" fn(*mut c_void, *const Pass, *mut Frame) -> i32,
    evaluate_pipeline: unsafe extern "C" fn(*mut c_void, *const Frame) -> i32,
    publish_output: unsafe extern "C" fn(*mut c_void, *const Pass) -> i32,
}

fn extent([left, top, right, bottom]: [i32; 4]) -> Option<[u32; 2]> {
    (left >= 0 && top >= 0 && right > left && bottom > top)
        .then(|| [(right - left) as u32, (bottom - top) as u32])
}

fn validate_pass(pass: &Pass) -> Result<[u32; 2], i32> {
    if pass.role != RSF_GAME_RENDER_SR
        || pass.camera_valid == 0
        || pass.color_input.is_null()
        || pass.color_output.is_null()
        || pass.depth.is_null()
        || pass.motion.is_null()
        || pass.render_rect[0] < 0
        || pass.render_rect[1] < 0
        || pass.output_rect[0] < 0
        || pass.output_rect[1] < 0
    {
        return Err(INVALID);
    }
    let source = &pass.camera;
    if source.struct_size < size_of::<rsf_camera_frame>() as u32
        || source.abi_version != RSF_GAME_CAMERA_ABI_VERSION
        || source.render_width == 0
        || source.render_height == 0
    {
        return Err(ABI_MISMATCH);
    }
    if extent(pass.render_rect) != Some([source.render_width, source.render_height]) {
        return Err(INVALID);
    }
    extent(pass.output_rect).ok_or(INVALID)
}

unsafe fn input_is_local(pass: &Pass, depth_layout: i32) -> bool {
    if depth_layout != 0 || pass.render_rect[0] != 0 || pass.render_rect[1] != 0 {
        return false;
    }
    let render = (pass.camera.render_width, pass.camera.render_height);
    let desc = unsafe { texture_desc(pass.color_input) };
    desc.format == RSF_D3D11_FORMAT_RGBA16_FLOAT
        && (desc.width, desc.height) == render
        && [pass.depth, pass.motion].iter().all(|&texture| {
            let desc = unsafe { texture_desc(texture) };
            (desc.width, desc.height) == render
        })
}

fn discard_history(result: i32) -> i32 {
    rsf_core_native_sr_reset(SR);
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_sr_evaluate(
    context: *mut c_void,
    pass: *const Pass,
    services: &Services,
) -> i32 {
    if context.is_null()
        || pass.is_null()
        || unsafe { pass.cast::<u32>().read() } < size_of::<Pass>() as u32
    {
        return INVALID;
    }
    let pass = unsafe { &*pass };
    let [output_width, output_height] = match validate_pass(pass) {
        Ok(extent) => extent,
        Err(code) => return code,
    };
    let camera = camera_for(SR, pass);

    match unsafe { (services.validate_device)(context, pass) } {
        0 => return INVALID,
        1 => {}
        _ => return RESOURCE_FAILED,
    }
    let depth_layout = unsafe { (services.depth_layout)(pass) };
    if depth_layout < 0 {
        return if depth_layout == -1 {
            INVALID
        } else {
            RESOURCE_FAILED
        };
    }
    let Some(_bindings) = (unsafe { SavedBindings::new(context, services.save, services.restore) })
    else {
        return RESOURCE_FAILED;
    };
    unsafe { (services.clear_predication)(context) };
    let resized = unsafe { (services.resize_output)(output_width, output_height) };
    if resized != 0 {
        return discard_history(resized);
    }

    let mut textures = [pass.color_input, pass.depth, pass.motion];
    if !unsafe { input_is_local(pass, depth_layout) }
        && unsafe { (services.prepare_regions)(context, pass, textures.as_mut_ptr()) } == 0
    {
        return RESOURCE_FAILED;
    }
    let mut frame = Frame {
        struct_size: size_of::<Frame>() as u32,
        abi_version: RSF_DLSS_PIPELINE_ABI_VERSION,
        scene_color: textures[0],
        depth: textures[1],
        game_motion: textures[2],
        exposure: unsafe { (services.exposure)(context, pass.exposure) },
        render_width: pass.camera.render_width,
        render_height: pass.camera.render_height,
        camera: &camera,
        ..unsafe { std::mem::zeroed() }
    };
    let mut masks = Masks {
        struct_size: size_of::<Masks>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    match unsafe { (services.take_translucency)(pass, &mut masks) } {
        0 => {}
        -1 => return RESOURCE_FAILED,
        _ => {
            frame.color_before_transparency = masks.color_before_transparency;
            frame.transparency_layer = masks.transparency_layer;
            frame.reactive_mask = masks.reactive;
            frame.transparency_mask = masks.coverage;
            frame.bias_mask = masks.bias;
            frame.motion_depth_layer = masks.motion_depth;
        }
    }
    if !pass.temporal_mask.is_null()
        && unsafe { (services.merge_mask)(context, pass, &mut frame) } == 0
    {
        return RESOURCE_FAILED;
    }

    let result = with_scene(pass, || unsafe {
        (services.evaluate_pipeline)(context, &frame)
    });
    if result != 0 {
        return discard_history(result);
    }
    // The provider output stays private until the current immediate stream
    // publishes it into the adapter's leased native destinations.
    if unsafe { (services.publish_output)(context, pass) } == 0 {
        return discard_history(RESOURCE_FAILED);
    }
    rsf_core_native_sr_commit(SR, pass);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_extents_refuse_before_subtraction() {
        assert_eq!(extent([1, 1, i32::MIN, 4]), None);
        assert_eq!(extent([i32::MIN, 0, i32::MAX, 4]), None);
        assert_eq!(extent([4, 2, 68, 34]), Some([64, 32]));
        assert_eq!(extent([0, 0, 1, 0]), None);
    }
}
