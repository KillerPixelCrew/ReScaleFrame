// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_output::{
    GraphicsCache, RestoreBindings, SaveBindings, SavedBindings, texture_desc,
};
use crate::d3d11_output_abi::{
    RSF_D3D11_FORMAT_RGBA16_FLOAT, rsf_d3d11_output_child_device, rsf_d3d11_output_is_immediate,
};
use crate::fg_scene::with_scene;
use crate::frame_assembly::rsf_assemble_dlss_frame;
use crate::native_sr_abi::{
    RSF_DLSS_PIPELINE_ABI_VERSION, rsf_dlss_pipeline_frame as Frame,
    rsf_motion_decode_params as DecodeParams, rsf_native_translucency_masks as Masks,
};
use crate::native_sr_state::{camera_for, commit_history, rsf_core_native_sr_reset};
use crate::render_abi::{
    rsf_dlss_frame as DlssFrame, rsf_frame_resources, rsf_motion_resolve_params,
};
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_RENDER_FG_INPUTS, RSF_GAME_RENDER_NO_FG,
    RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR, rsf_camera_frame, rsf_game_render_pass as Pass,
};
use std::ffi::{c_char, c_void};
use std::ptr::NonNull;

const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_READY: i32 = -3;
const RESOURCE_FAILED: i32 = -7;
const SR: u32 = 0;
const FG: u32 = 1;
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

    if unsafe { rsf_d3d11_output_is_immediate(context) } == 0 {
        return INVALID;
    }
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
    commit_history(SR, pass);
    0
}

#[repr(C)]
struct FgServices {
    create_decode: unsafe extern "C" fn(Texture, u32, u32) -> Texture,
    create_resolve: unsafe extern "C" fn(Texture, u32, u32) -> Texture,
    decode: unsafe extern "C" fn(Texture, Texture, Texture, *const DecodeParams) -> i32,
    resolve: unsafe extern "C" fn(
        Texture,
        Texture,
        Texture,
        Texture,
        *const rsf_motion_resolve_params,
    ) -> i32,
    decoded_texture: unsafe extern "C" fn(Texture) -> Texture,
    depth: unsafe extern "C" fn(Texture) -> Texture,
    motion: unsafe extern "C" fn(Texture) -> Texture,
    destroy_decode: unsafe extern "C" fn(Texture),
    destroy_resolve: unsafe extern "C" fn(Texture),
    stage: unsafe extern "C" fn(*const c_char, u64, u64, i32),
    capture: unsafe extern "C" fn(Texture, *const DlssFrame, u64, u64) -> i32,
    refuse: unsafe extern "C" fn(*const Pass),
}

struct MotionPass {
    handle: NonNull<c_void>,
    destroy: unsafe extern "C" fn(Texture),
}

impl MotionPass {
    fn new(handle: Texture, destroy: unsafe extern "C" fn(Texture)) -> Option<Self> {
        Some(Self {
            handle: NonNull::new(handle)?,
            destroy,
        })
    }

    fn raw(&self) -> Texture {
        self.handle.as_ptr()
    }
}

impl Drop for MotionPass {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.raw()) };
    }
}

struct FgInputs {
    decode: MotionPass,
    resolve: MotionPass,
    extent: [u32; 2],
    device: ComRef,
}

// SAFETY: these D3D11 passes are used serially on the graphics owner, and
// destroyed only after its work drains.
unsafe impl Send for FgInputs {}

static FG_INPUTS: GraphicsCache<Option<FgInputs>> = GraphicsCache::new(None);

unsafe fn capture_on_stream(
    context: Texture,
    pass: &Pass,
    motion: &DecodeParams,
    sr: &Services,
    fg: &FgServices,
) -> Option<()> {
    let source = &pass.camera;
    let _bindings = unsafe { SavedBindings::new(context, sr.save, sr.restore) }?;
    unsafe { (sr.clear_predication)(context) };
    let device = unsafe { ComRef::from_owned(rsf_d3d11_output_child_device(context)) }?;
    let stage = |name: &std::ffi::CStr| unsafe {
        (fg.stage)(name.as_ptr(), pass.source_frame_id, pass.native_frame, 0)
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
                rsf_core_native_sr_reset(FG);
                let decode = MotionPass::new(
                    unsafe { (fg.create_decode)(device.as_ptr(), width, height) },
                    fg.destroy_decode,
                )?;
                let resolve = MotionPass::new(
                    unsafe { (fg.create_resolve)(device.as_ptr(), width, height) },
                    fg.destroy_resolve,
                )?;
                cache.insert(FgInputs {
                    decode,
                    resolve,
                    extent: render,
                    device,
                })
            };
            stage(c"normalize-regions-decode");
            let mut textures: [Texture; 3] = [std::ptr::null_mut(); 3];
            if unsafe { (sr.prepare_regions)(context, pass, textures.as_mut_ptr()) } == 0 {
                return None;
            }
            let [color, depth, vectors] = textures;
            if unsafe { (fg.decode)(inputs.decode.raw(), context, vectors, motion) } == 0 {
                return None;
            }
            let resolve = rsf_motion_resolve_params {
                struct_size: size_of::<rsf_motion_resolve_params>() as u32,
                clip_to_previous: source.clip_to_previous_clip,
                decoded_to_pixels: [
                    -(width as f32) * pass.motion_to_uv[0],
                    -(height as f32) * pass.motion_to_uv[1],
                ],
                has_sentinel: u32::from(
                    motion.zero_means_unwritten != 0 || motion.validity_enabled != 0,
                ),
                sentinel: motion.invalid_value,
                depth_layer: std::ptr::null_mut(),
                jitter_pixels: source.jitter_pixels,
            };
            stage(c"normalize-resolve");
            let decoded = unsafe { (fg.decoded_texture)(inputs.decode.raw()) };
            if unsafe { (fg.resolve)(inputs.resolve.raw(), context, decoded, depth, &resolve) } == 0
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
                depth: unsafe { (fg.depth)(inputs.resolve.raw()) },
                motion: unsafe { (fg.motion)(inputs.resolve.raw()) },
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
                (fg.capture)(context, &frame, pass.source_frame_id, pass.native_frame)
            });
            if accepted == 0 {
                return None;
            }
            commit_history(FG, pass);
            Some(())
        })
        .flatten()
}

unsafe fn capture_validated(
    context: Texture,
    pass: &Pass,
    motion: &DecodeParams,
    sr: &Services,
    fg: &FgServices,
) -> i32 {
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
        (fg.stage)(
            c"normalize-enter".as_ptr(),
            pass.source_frame_id,
            pass.native_frame,
            0,
        )
    };
    match unsafe { (sr.validate_device)(context, pass) } {
        0 => return INVALID,
        1 => {}
        _ => return NOT_READY,
    }
    match unsafe { (sr.depth_layout)(pass) } {
        -1 => return INVALID,
        n if n < 0 => return NOT_READY,
        _ => {}
    }
    if unsafe { capture_on_stream(context, pass, motion, sr, fg) }.is_some() {
        0
    } else {
        NOT_READY
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_native_sr_capture_fg(
    context: Texture,
    pass: *const Pass,
    motion: *const DecodeParams,
    sr: &Services,
    fg: &FgServices,
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
    let result = unsafe { capture_validated(context, pass, &*motion, sr, fg) };
    if result != 0 && pass.source_frame_id != 0 {
        // A refused explicit lease invalidates matching prior inputs. It never
        // borrows a different source's inputs, and the enclosing scene survives.
        let refused = Pass {
            flags: pass.flags | RSF_GAME_RENDER_NO_FG,
            ..*pass
        };
        with_scene(&refused, || unsafe { (fg.refuse)(&refused) });
    }
    result
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_native_sr_fg_inputs_release() {
    FG_INPUTS.with(|cache| *cache = None);
    rsf_core_native_sr_reset(FG);
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
