// SPDX-License-Identifier: MIT
#[cfg(not(test))]
mod platform;
#[cfg(not(test))]
use crate::{
    graphics_d3d11::{rsf_d3d11_state_restore, rsf_d3d11_state_save},
    motion_decode::{rsf_motion_decode_run, rsf_motion_decode_texture},
    motion_resolve::{rsf_motion_resolve_depth, rsf_motion_resolve_motion, rsf_motion_resolve_run},
    native_exposure::rsf_native_scalar_exposure,
    native_fg::{rsf_native_fg_inputs_ready, rsf_native_fg_scene},
    native_fg_cpu::platform::rsf_native_fg_stage,
    native_translucency::rsf_native_translucency_take,
};

use crate::com_ref::ComRef;
use crate::d3d11_output::GraphicsCache;
#[cfg(not(test))]
use crate::d3d11_output::{SavedBindings, texture_desc};
#[cfg(not(test))]
use crate::fg_scene::with_scene;
#[cfg(not(test))]
use crate::frame_assembly::rsf_assemble_dlss_frame;
#[cfg(not(test))]
use crate::motion_resolve_params::{dense_resolve_params, has_motion_sentinel};
use crate::native_composition::rsf_native_composition_release;
use crate::native_cpu::rsf_native_cpu_release;
use crate::native_exposure::rsf_core_native_exposure_release;
use crate::native_regions::rsf_native_regions_release;
use crate::native_reinsert::rsf_native_reinsert_release;
use crate::native_scene::rsf_native_scene_reset;
use crate::native_sr_state::{camera_for, commit_history};
use crate::native_sr_state::{clear_surface, reset_history, rsf_native_sr_set_enabled};
use crate::native_translucency::rsf_native_translucency_release;
use crate::native_window::rsf_native_window_reset;
use crate::owned_handle::OwnedHandle;
#[cfg(not(test))]
use crate::sys::rsf_native_translucency_masks as Masks;
#[cfg(not(test))]
use crate::sys::{
    RSF_D3D11_FORMAT_RGBA16_FLOAT, rsf_d3d11_output_child_device, rsf_d3d11_output_is_immediate,
};
#[cfg(not(test))]
use crate::sys::{
    RSF_DLSS_PIPELINE_ABI_VERSION, rsf_dlss_pipeline_frame as Frame,
    rsf_motion_decode_params as DecodeParams,
};
#[cfg(not(test))]
use crate::sys::{rsf_dlss_frame as DlssFrame, rsf_frame_resources};
#[cfg(test)]
use rsf_game_sys::rsf_game_render_pass as Pass;
#[cfg(not(test))]
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_RENDER_FG_INPUTS, RSF_GAME_RENDER_NO_FG,
    RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR, rsf_camera_frame, rsf_game_render_pass as Pass,
};
use std::ffi::c_void;
#[cfg(not(test))]
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;
#[cfg(not(test))]
use windows::core::Interface;

#[cfg(not(test))]
const INVALID: i32 = -1;
#[cfg(not(test))]
const ABI_MISMATCH: i32 = -2;
#[cfg(not(test))]
const NOT_READY: i32 = -3;
const RESOURCE_FAILED: i32 = -7;
const SR: u32 = 0;
const FG: u32 = 1;
type Texture = *mut c_void;

#[cfg(not(test))]
unsafe fn validate_device(context: *mut c_void, pass: &Pass) -> bool {
    let context = unsafe { crate::d3d11_d3d12::borrow::<ID3D11DeviceContext>(&context) };
    let Ok(device) = (unsafe { context.GetDevice() }) else {
        return false;
    };
    let Some(identity) = (unsafe { ComRef::identity(device.as_raw()) }) else {
        return false;
    };
    let textures = [
        pass.color_input,
        pass.color_output,
        pass.depth,
        pass.motion,
        pass.exposure,
        pass.color_output_readable,
        pass.temporal_mask,
    ];
    textures
        .into_iter()
        .filter(|texture| !texture.is_null())
        .all(|texture| {
            unsafe { ComRef::from_owned(rsf_d3d11_output_child_device(texture)) }
                .and_then(|owner| unsafe { ComRef::identity(owner.as_ptr()) })
                .is_some_and(|other| other.as_ptr() == identity.as_ptr())
        })
}

#[cfg(not(test))]
unsafe fn clear_predication(context: *mut c_void) {
    let context = unsafe { crate::d3d11_d3d12::borrow::<ID3D11DeviceContext>(&context) };
    unsafe { context.SetPredication(None, false) };
}

fn extent([left, top, right, bottom]: [i32; 4]) -> Option<[u32; 2]> {
    (left >= 0 && top >= 0 && right > left && bottom > top)
        .then(|| [(right - left) as u32, (bottom - top) as u32])
}

#[cfg(not(test))]
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

#[cfg(not(test))]
unsafe fn input_is_local(pass: &Pass, depth_domains: bool) -> bool {
    if depth_domains || pass.render_rect[0] != 0 || pass.render_rect[1] != 0 {
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

fn discard_history(owner: u32, result: i32) -> i32 {
    reset_history(owner);
    result
}

type PrepareRegions = unsafe fn(Texture, &Pass, &mut [Texture; 3]) -> bool;

/// Decodes the region preparation once for SR and explicit FG: the three textures on
/// success, `RESOURCE_FAILED` for a refusal or an incomplete publication.
unsafe fn prepare_regions(
    prepare: PrepareRegions,
    context: Texture,
    pass: &Pass,
) -> Result<[Texture; 3], i32> {
    let mut textures = [std::ptr::null_mut(); 3];
    if unsafe { prepare(context, pass, &mut textures) }
        && textures.iter().all(|texture| !texture.is_null())
    {
        Ok(textures)
    } else {
        Err(RESOURCE_FAILED)
    }
}

/// D3D11 host entry. Every native write of this call is recorded on the immediate
/// context before it returns, so the receipt is always ordered.
#[cfg(not(test))]
pub(crate) unsafe extern "C" fn rsf_native_sr_evaluate_ordered(
    context: *mut c_void,
    pass: *const Pass,
    ordered: *mut u32,
) -> i32 {
    let result = unsafe { evaluate(context, pass) };
    if !ordered.is_null() {
        unsafe { ordered.write(1) };
    }
    result
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_sr_evaluate(context: *mut c_void, pass: *const Pass) -> i32 {
    unsafe { evaluate(context, pass) }
}

#[cfg(not(test))]
unsafe fn evaluate(context: *mut c_void, pass: *const Pass) -> i32 {
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

    if unsafe { rsf_d3d11_output_is_immediate(context) } == 0 {
        return INVALID;
    }
    if !unsafe { validate_device(context, pass) } {
        return INVALID;
    }
    let Some(depth_domains) = (unsafe { platform::depth_layout(pass) }) else {
        return INVALID;
    };
    let Some(_bindings) =
        (unsafe { SavedBindings::new(context, rsf_d3d11_state_save, rsf_d3d11_state_restore) })
    else {
        return RESOURCE_FAILED;
    };
    unsafe { clear_predication(context) };
    let resized = unsafe { platform::resize_output(output_width, output_height) };
    if resized != 0 {
        return discard_history(SR, resized);
    }

    let textures = if unsafe { input_is_local(pass, depth_domains) } {
        [pass.color_input, pass.depth, pass.motion]
    } else {
        match unsafe { prepare_regions(platform::prepare, context, pass) } {
            Ok(textures) => textures,
            Err(code) => return code,
        }
    };
    let mut frame = Frame {
        struct_size: size_of::<Frame>() as u32,
        abi_version: RSF_DLSS_PIPELINE_ABI_VERSION,
        scene_color: textures[0],
        depth: textures[1],
        game_motion: textures[2],
        exposure: unsafe { rsf_native_scalar_exposure(context, pass.exposure) },
        render_width: pass.camera.render_width,
        render_height: pass.camera.render_height,
        camera: &camera,
        ..unsafe { std::mem::zeroed() }
    };
    let mut masks = Masks {
        struct_size: size_of::<Masks>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    match unsafe { rsf_native_translucency_take(pass, &mut masks) } {
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
    if !pass.temporal_mask.is_null() && !unsafe { platform::merge_mask(context, pass, &mut frame) }
    {
        return RESOURCE_FAILED;
    }

    let result = with_scene(pass, || unsafe { platform::run_frame(context, &frame) });
    if result != 0 {
        return discard_history(SR, result);
    }
    // The provider output stays private until the current immediate stream
    // publishes it into the adapter's leased native destinations.
    if !unsafe { platform::publish_output(context, pass) } {
        return discard_history(SR, RESOURCE_FAILED);
    }
    commit_history(SR, pass);
    0
}

#[cfg_attr(test, allow(dead_code))]
struct FgInputs {
    decode: OwnedHandle,
    resolve: OwnedHandle,
    extent: [u32; 2],
    device: ComRef,
}

// SAFETY: these D3D11 passes are used serially on the graphics owner, and
// destroyed only after its work drains.
unsafe impl Send for FgInputs {}

static FG_INPUTS: GraphicsCache<Option<FgInputs>> = GraphicsCache::new(None);

#[cfg(not(test))]
unsafe fn capture_on_stream(context: Texture, pass: &Pass, motion: &DecodeParams) -> Option<()> {
    let source = &pass.camera;
    let _bindings =
        unsafe { SavedBindings::new(context, rsf_d3d11_state_save, rsf_d3d11_state_restore) }?;
    unsafe { clear_predication(context) };
    let device = unsafe { ComRef::from_owned(rsf_d3d11_output_child_device(context)) }?;
    let stage = |name: &std::ffi::CStr| unsafe {
        rsf_native_fg_stage(name.as_ptr(), pass.source_frame_id, pass.native_frame, 0)
    };
    let render = [source.render_width, source.render_height];
    let [width, height] = render;
    FG_INPUTS
        .with(|cache| {
            let inputs = if let Some(inputs) = cache.as_ref().filter(|inputs| {
                inputs.device.as_ptr() == device.as_ptr() && inputs.extent == render
            }) {
                inputs
            } else {
                *cache = None;
                reset_history(FG);
                let decode = OwnedHandle::new(
                    unsafe { platform::create_decode(device.as_ptr(), width, height) },
                    platform::destroy_decode,
                )?;
                let resolve = OwnedHandle::new(
                    unsafe { platform::create_resolve(device.as_ptr(), width, height) },
                    platform::destroy_resolve,
                )?;
                cache.insert(FgInputs {
                    decode,
                    resolve,
                    extent: render,
                    device,
                })
            };
            stage(c"normalize-regions-decode");
            let [color, depth, vectors] =
                unsafe { prepare_regions(platform::prepare, context, pass) }.ok()?;
            if unsafe {
                rsf_motion_decode_run(inputs.decode.get().cast(), context, vectors, motion)
            } != 0
            {
                return None;
            }
            let resolve = dense_resolve_params(
                source.clip_to_previous_clip,
                [width, height],
                pass.motion_to_uv,
                (motion.invalid_value, has_motion_sentinel(motion)),
                std::ptr::null_mut(),
                source.jitter_pixels,
            );
            stage(c"normalize-resolve");
            let decoded = unsafe { rsf_motion_decode_texture(inputs.decode.get().cast()) };
            if unsafe {
                rsf_motion_resolve_run(
                    inputs.resolve.get().cast(),
                    context,
                    decoded,
                    depth,
                    &resolve,
                )
            } == 0
            {
                return None;
            }
            let mut camera = camera_for(FG, pass);
            camera.motion_decoded = 1;
            camera.camera_motion_included = 1;
            camera.motion_scale = [1.0 / width as f32, 1.0 / height as f32];
            let resources = rsf_frame_resources {
                struct_size: size_of::<rsf_frame_resources>() as u32,
                color_in: color,
                color_out: color,
                depth: unsafe { rsf_motion_resolve_depth(inputs.resolve.get().cast()) },
                motion: unsafe { rsf_motion_resolve_motion(inputs.resolve.get().cast()) },
                exposure: std::ptr::null_mut(),
                render_width: width,
                render_height: height,
                output_width: source.output_width,
                output_height: source.output_height,
                quality: 0, // Native, with only normalized inputs consumed by FG.
            };
            let mut frame = DlssFrame {
                struct_size: size_of::<DlssFrame>() as u32,
                ..unsafe { std::mem::zeroed() }
            };
            if unsafe { rsf_assemble_dlss_frame(&camera, &resources, &mut frame) } != 0 {
                return None;
            }
            let mut captured = *pass;
            if camera.reset != 0 {
                captured.flags |= RSF_GAME_RENDER_RESET;
            }
            let accepted = with_scene(&captured, || unsafe {
                stage(c"normalize-capture");
                platform::capture_fg(context, &frame);
                stage(c"normalize-captured");
                rsf_native_fg_inputs_ready(pass.source_frame_id)
            });
            if accepted == 0 {
                return None;
            }
            commit_history(FG, pass);
            Some(())
        })
        .flatten()
}

#[cfg(not(test))]
unsafe fn capture_validated(context: Texture, pass: &Pass, motion: &DecodeParams) -> i32 {
    if pass.source_frame_id == 0
        || pass.camera_valid == 0
        || pass.color_input.is_null()
        || pass.depth.is_null()
        || pass.motion.is_null()
        || pass.session_id == 0
        || pass.submission_id == 0
        || pass.motion_camera_included == 0
        || pass.motion_to_uv.contains(&0.0)
    {
        return NOT_READY;
    }
    let source = &pass.camera;
    if source.struct_size < size_of::<rsf_camera_frame>() as u32
        || source.abi_version != RSF_GAME_CAMERA_ABI_VERSION
    {
        return ABI_MISMATCH;
    }
    if extent(pass.render_rect) != Some([source.render_width, source.render_height])
        || source.output_width == 0
        || source.output_height == 0
        || unsafe { rsf_d3d11_output_is_immediate(context) } == 0
    {
        return INVALID;
    }
    unsafe {
        rsf_native_fg_stage(
            c"normalize-enter".as_ptr(),
            pass.source_frame_id,
            pass.native_frame,
            0,
        )
    };
    if !unsafe { validate_device(context, pass) }
        || unsafe { platform::depth_layout(pass) }.is_none()
    {
        return INVALID;
    }
    if unsafe { capture_on_stream(context, pass, motion) }.is_some() {
        0
    } else {
        NOT_READY
    }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_sr_capture_fg(
    context: Texture,
    pass: *const Pass,
    motion: *const DecodeParams,
) -> i32 {
    if context.is_null()
        || pass.is_null()
        || motion.is_null()
        || unsafe { pass.cast::<u32>().read() } < size_of::<Pass>() as u32
        || unsafe { motion.cast::<u32>().read() } < size_of::<DecodeParams>() as u32
    {
        return INVALID;
    }
    let pass = unsafe { &*pass };
    if pass.role != RSF_GAME_RENDER_FG_INPUTS {
        return INVALID;
    }
    let result = unsafe { capture_validated(context, pass, &*motion) };
    if result != 0 && pass.source_frame_id != 0 {
        // A refused explicit lease invalidates matching prior inputs. It never
        // borrows a different source's inputs, and the enclosing scene survives.
        let refused = Pass {
            flags: pass.flags | RSF_GAME_RENDER_NO_FG,
            ..*pass
        };
        with_scene(&refused, || unsafe { rsf_native_fg_scene(&refused) });
    }
    result
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_sr_release_resources() {
    rsf_native_scene_reset();
    rsf_native_window_reset();
    rsf_native_cpu_release();
    rsf_native_composition_release();
    rsf_native_sr_set_enabled(0);
    rsf_core_native_exposure_release();
    rsf_native_regions_release();
    FG_INPUTS.with(|cache| *cache = None);
    reset_history(FG);
    rsf_native_reinsert_release();
    reset_history(SR);
    rsf_native_translucency_release();
    clear_surface();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_refusal_keeps_history_and_discard_resets_one_owner() {
        unsafe fn refused(_: Texture, _: &Pass, _: &mut [Texture; 3]) -> bool {
            false
        }
        unsafe fn incomplete(_: Texture, _: &Pass, out: &mut [Texture; 3]) -> bool {
            out[0] = std::ptr::dangling_mut();
            true
        }
        let mut pass: Pass = unsafe { std::mem::zeroed() };
        pass.session_id = 1;
        pass.native_frame = 1;
        commit_history(SR, &pass);
        commit_history(FG, &pass);
        pass.native_frame = 2;
        assert_eq!(camera_for(SR, &pass).reset, 0);
        assert_eq!(camera_for(FG, &pass).reset, 0);

        assert_eq!(
            unsafe { prepare_regions(incomplete, std::ptr::null_mut(), &pass) },
            Err(RESOURCE_FAILED)
        );
        assert_eq!(
            unsafe { prepare_regions(refused, std::ptr::null_mut(), &pass) },
            Err(RESOURCE_FAILED)
        );
        assert_eq!(camera_for(SR, &pass).reset, 0);
        assert_eq!(camera_for(FG, &pass).reset, 0);
        assert_eq!(discard_history(FG, RESOURCE_FAILED), RESOURCE_FAILED);
        assert_eq!(camera_for(SR, &pass).reset, 0);
        assert_eq!(camera_for(FG, &pass).reset, 1);
        reset_history(SR);
    }

    #[test]
    fn invalid_extents_refuse_before_subtraction() {
        assert_eq!(extent([1, 1, i32::MIN, 4]), None);
        assert_eq!(extent([i32::MIN, 0, i32::MAX, 4]), None);
        assert_eq!(extent([4, 2, 68, 34]), Some([64, 32]));
        assert_eq!(extent([0, 0, 1, 0]), None);
    }
}
