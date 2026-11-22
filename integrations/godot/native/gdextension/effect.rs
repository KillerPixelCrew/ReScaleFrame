// SPDX-License-Identifier: MIT
//! `ReScaleFrameEffect`: a `CompositorEffect` with the `POST_TRANSPARENT` callback, resolved
//! color and depth and motion vectors. Each instance owns one `effect.rs` probe. The callback's
//! scene objects, RIDs and readback bytes live in `Call` for that callback only; the readback's
//! shader, uniform and sampler objects live in `Readback` through the synchronous capture
//! sequence that `rsf_godot_effect_record_motion` drives.
use super::api::{self, Bind, GString, Godot, PackedBytes, Ref, StringName, TypedArray, arg};
use super::sys::*;
use super::{Dictionary, EFFECT_CLASS, Sink};
use crate::effect::Effect;
use crate::extension_abi::*;
use crate::platform::{
    EFFECT_MOTION_FORMAT, EFFECT_VERSION, EffectFrame, EffectLeaves, EffectReadback,
    EffectResources,
};
use std::ffi::{CStr, c_char, c_void};

/// `CompositorEffect::EFFECT_CALLBACK_TYPE_POST_TRANSPARENT`.
const POST_TRANSPARENT: i64 = 4;
/// `_render_callback(int, RenderData)` in the pinned 4.7 API.
const RENDER_CALLBACK_HASH: u32 = 2153422729;
/// `RenderingDevice::DATA_FORMAT_R16G16_SFLOAT`, the velocity format `effect.rs` admits.
const DATA_FORMAT_R16G16_SFLOAT: i32 = 82;
const _: () = assert!(DATA_FORMAT_R16G16_SFLOAT == EFFECT_MOTION_FORMAT);
const SHADER_STAGE_COMPUTE: i64 = 4;
const UNIFORM_TYPE_SAMPLER_WITH_TEXTURE: i64 = 1;
const UNIFORM_TYPE_STORAGE_BUFFER: i64 = 8;
const DRIVER_RESOURCE_LOGICAL_DEVICE: i64 = 0;
const DRIVER_RESOURCE_COMMAND_QUEUE: i64 = 3;
const DRIVER_RESOURCE_TEXTURE: i64 = 5;

unsafe extern "C" {
    /// The CMake project version (`runtime_platform.c`).
    fn rsf_godot_version() -> *const c_char;
}

/// The engine owns the object; the instance is the probe it lends to each callback and method.
struct Instance {
    probe: Effect,
}

/// `GDExtensionClassCreateInstance3`: the parent object, then the godot-cpp constructor's flags.
pub(super) unsafe extern "C" fn create(_: *mut c_void, notify_postinitialize: GdBool) -> ObjectPtr {
    let Some(godot) = api::godot() else {
        return std::ptr::null_mut();
    };
    let object = godot.construct(c"CompositorEffect");
    if object.is_null() {
        return object;
    }
    let instance = Box::into_raw(Box::new(Instance {
        probe: Effect::default(),
    }));
    let class = StringName::new(godot, EFFECT_CLASS);
    unsafe { (godot.interface.object_set_instance)(object, class.ptr(), instance.cast()) };
    let (kind, enabled) = (POST_TRANSPARENT, 1u8);
    unsafe {
        godot.call(Bind::SET_EFFECT_CALLBACK_TYPE, object, &[arg(&kind)]);
        godot.call(Bind::SET_ACCESS_RESOLVED_COLOR, object, &[arg(&enabled)]);
        godot.call(Bind::SET_ACCESS_RESOLVED_DEPTH, object, &[arg(&enabled)]);
        godot.call(Bind::SET_NEEDS_MOTION_VECTORS, object, &[arg(&enabled)]);
    }
    if notify_postinitialize != 0 {
        godot.postinitialize(object);
    }
    object
}

pub(super) unsafe extern "C" fn free(_: *mut c_void, instance: *mut c_void) {
    if !instance.is_null() {
        // SAFETY: `create` boxed this instance and the engine frees it once.
        drop(unsafe { Box::from_raw(instance.cast::<Instance>()) });
    }
}

pub(super) unsafe extern "C" fn get_virtual(
    _: *mut c_void,
    name: *const c_void,
    hash: u32,
) -> Option<CallVirtual> {
    let godot = api::godot()?;
    let expected = StringName::new(godot, c"_render_callback");
    (hash == RENDER_CALLBACK_HASH && unsafe { expected.same(name) })
        .then_some(render_callback as CallVirtual)
}

/// `_render_callback(effect_callback_type, render_data)`.
unsafe extern "C" fn render_callback(instance: *mut c_void, args: *const ConstTypePtr, _: TypePtr) {
    let (Some(godot), Some(instance)) = (api::godot(), unsafe {
        instance.cast::<Instance>().as_ref()
    }) else {
        return;
    };
    let kind = unsafe { (*args).cast::<i64>().read_unaligned() };
    let data = unsafe { (*args.add(1)).cast::<ObjectPtr>().read_unaligned() };
    if kind != POST_TRANSPARENT || data.is_null() {
        return;
    }
    let mut call = Call {
        godot,
        data,
        buffers: None,
        device: std::ptr::null_mut(),
        color: 0,
        depth: 0,
        motion: 0,
        bytes: None,
    };
    instance.probe.render(&LEAVES, (&raw mut call).cast());
}

/// `get_status()`.
///
/// # Safety
/// `instance` is a live instance from `create`.
pub(super) unsafe fn get_status(godot: &'static Godot, instance: *mut c_void) -> Dictionary {
    let mut dictionary = Dictionary::new(godot);
    let instance = unsafe { &*instance.cast::<Instance>() };
    let rendering = godot.rendering_server();
    let mut sink = Sink {
        godot,
        dictionary: &mut dictionary,
        rendering,
    };
    let (user, emit) = sink.emitter();
    unsafe {
        crate::effect::rsf_godot_effect_properties(&instance.probe, rsf_godot_version(), user, emit)
    };
    dictionary
}

/// `request_motion_capture()`.
///
/// # Safety
/// `instance` is a live instance from `create`.
pub(super) unsafe fn request_motion_capture(instance: *mut c_void) {
    unsafe { &*instance.cast::<Instance>() }
        .probe
        .request_capture();
}

/// One `POST_TRANSPARENT` callback's borrowed scene state and owned readback bytes.
struct Call {
    godot: &'static Godot,
    data: ObjectPtr,
    buffers: Option<Ref>,
    device: ObjectPtr,
    color: u64,
    depth: u64,
    motion: u64,
    bytes: Option<PackedBytes>,
}

static LEAVES: EffectLeaves = EffectLeaves {
    struct_size: size_of::<EffectLeaves>() as u32,
    version: EFFECT_VERSION,
    frame: leaf_frame,
    resources: leaf_resources,
    read_motion: leaf_read_motion,
};

fn layer(godot: &Godot, method: Bind, buffers: ObjectPtr) -> Option<u64> {
    let (index, msaa) = (0i64, 0u8);
    unsafe { godot.value(method, buffers, &[arg(&index), arg(&msaa)], 0u64) }
}

fn texture_valid(godot: &Godot, device: ObjectPtr, texture: u64) -> Option<u32> {
    unsafe { godot.value(Bind::TEXTURE_IS_VALID, device, &[arg(&texture)], 0u8) }.map(u32::from)
}

unsafe extern "C" fn leaf_frame(call: *mut c_void, frame: *mut EffectFrame) -> u32 {
    let (call, frame) = unsafe { (&mut *call.cast::<Call>(), &mut *frame) };
    read_frame(call, frame).is_some().into()
}

fn read_frame(call: &mut Call, frame: &mut EffectFrame) -> Option<()> {
    let godot = call.godot;
    // Only RenderSceneBuffersRD converts to the Ref the scene reads use.
    call.buffers = unsafe { godot.reference(Bind::GET_RENDER_SCENE_BUFFERS, call.data, &[]) }
        .filter(|buffers| godot.is_class(buffers.object(), c"RenderSceneBuffersRD"));
    let scene = unsafe {
        godot.value(
            Bind::GET_RENDER_SCENE_DATA,
            call.data,
            &[],
            std::ptr::null_mut::<c_void>(),
        )
    }?;
    let rendering = godot.rendering_server();
    let device = if rendering.is_null() {
        std::ptr::null_mut()
    } else {
        unsafe {
            godot.value(
                Bind::GET_RENDERING_DEVICE,
                rendering,
                &[],
                std::ptr::null_mut::<c_void>(),
            )
        }?
    };
    let buffers = call.buffers.as_ref().map(Ref::object);
    let (Some(buffers), false, false) = (buffers, device.is_null(), scene.is_null()) else {
        return None;
    };
    call.device = device;
    frame.render_target = unsafe { godot.value(Bind::GET_RENDER_TARGET, buffers, &[], 0u64) }?;
    frame.views = unsafe { godot.value(Bind::GET_VIEW_COUNT, buffers, &[], 0i64) }? as u32;
    let [internal_width, internal_height] =
        unsafe { godot.value(Bind::GET_INTERNAL_SIZE, buffers, &[], [0i32; 2]) }?;
    let [output_width, output_height] =
        unsafe { godot.value(Bind::GET_TARGET_SIZE, buffers, &[], [0i32; 2]) }?;
    frame.internal_width = internal_width;
    frame.internal_height = internal_height;
    frame.output_width = output_width;
    frame.output_height = output_height;
    // Transform3D is basis rows then origin, Projection is columns: both copy in memory order.
    let camera = unsafe { godot.value(Bind::GET_CAM_TRANSFORM, scene, &[], [0f32; 12]) }?;
    let projection = unsafe { godot.value(Bind::GET_CAM_PROJECTION, scene, &[], [0f32; 16]) }?;
    frame.camera_transform = camera.map(f64::from);
    frame.jittered_projection = projection.map(f64::from);
    call.color = layer(godot, Bind::GET_COLOR_LAYER, buffers)?;
    call.depth = layer(godot, Bind::GET_DEPTH_LAYER, buffers)?;
    call.motion = layer(godot, Bind::GET_VELOCITY_LAYER, buffers)?;
    frame.color_valid = texture_valid(godot, device, call.color)?;
    frame.depth_valid = texture_valid(godot, device, call.depth)?;
    frame.motion_valid = texture_valid(godot, device, call.motion)?;
    Some(())
}

unsafe extern "C" fn leaf_resources(call: *mut c_void, resources: *mut EffectResources) -> u32 {
    let (call, resources) = unsafe { (&*call.cast::<Call>(), &mut *resources) };
    read_resources(call, resources).is_some().into()
}

fn read_resources(call: &Call, resources: &mut EffectResources) -> Option<()> {
    let godot = call.godot;
    let format = |texture: u64| -> Option<i32> {
        let format =
            unsafe { godot.reference(Bind::TEXTURE_GET_FORMAT, call.device, &[arg(&texture)]) }?;
        unsafe { godot.value(Bind::TEXTURE_FORMAT_GET_FORMAT, format.object(), &[], 0i64) }
            .map(|value| value as i32)
    };
    let driver = |resource: i64, rid: u64| -> Option<bool> {
        let index = 0u64;
        unsafe {
            godot.value(
                Bind::GET_DRIVER_RESOURCE,
                call.device,
                &[arg(&resource), arg(&rid), arg(&index)],
                0u64,
            )
        }
        .map(|handle| handle != 0)
    };
    resources.color_format = format(call.color)?;
    resources.depth_format = format(call.depth)?;
    resources.motion_format = format(call.motion)?;
    resources.native_device = driver(DRIVER_RESOURCE_LOGICAL_DEVICE, 0)?.into();
    resources.native_queue = driver(DRIVER_RESOURCE_COMMAND_QUEUE, 0)?.into();
    resources.native_textures = (driver(DRIVER_RESOURCE_TEXTURE, call.color)?
        && driver(DRIVER_RESOURCE_TEXTURE, call.depth)?
        && driver(DRIVER_RESOURCE_TEXTURE, call.motion)?)
    .into();
    Some(())
}

unsafe extern "C" fn leaf_read_motion(
    call: *mut c_void,
    plan: *const EffectReadback,
    data: *mut *const u8,
    size: *mut u64,
) -> u32 {
    let call = unsafe { &mut *call.cast::<Call>() };
    unsafe {
        *data = std::ptr::null();
        *size = 0;
    }
    let mut readback = Readback::new(call.godot, call.device, call.motion);
    let calls = rsf_godot_motion_calls {
        compile: Some(compile_motion),
        prepare: Some(prepare_motion),
        dispatch: Some(dispatch_motion),
        read: Some(read_motion),
        free_resource: Some(free_motion_resource),
    };
    let recorded = unsafe {
        crate::effect::rsf_godot_effect_record_motion((&raw mut readback).cast(), plan, &calls)
    } != 0;
    // The PackedByteArray owns the CPU copy until the callback returns.
    call.bytes = readback.bytes.take();
    drop(readback);
    if !recorded {
        return 0;
    }
    if let Some(bytes) = &call.bytes {
        let (bytes, count) = bytes.data();
        unsafe {
            *data = bytes;
            *size = count;
        }
    }
    1
}

/// The capture's SDK objects and RIDs. Its references release after the RIDs were freed.
struct Readback {
    godot: &'static Godot,
    device: ObjectPtr,
    motion: u64,
    shader: u64,
    pipeline: u64,
    sampler: u64,
    buffer: u64,
    uniform_set: u64,
    bytes: Option<PackedBytes>,
    // Dropped in this order, the reverse of the former C++ member order.
    uniforms: Option<TypedArray>,
    buffer_uniform: Option<Ref>,
    texture_uniform: Option<Ref>,
    sampler_state: Option<Ref>,
    spirv: Option<Ref>,
    source: Option<Ref>,
}

impl Readback {
    fn new(godot: &'static Godot, device: ObjectPtr, motion: u64) -> Self {
        Self {
            godot,
            device,
            motion,
            shader: 0,
            pipeline: 0,
            sampler: 0,
            buffer: 0,
            uniform_set: 0,
            bytes: None,
            uniforms: None,
            buffer_uniform: None,
            texture_uniform: None,
            sampler_state: None,
            spirv: None,
            source: None,
        }
    }

    fn rid(&self, method: Bind, args: &[ConstTypePtr]) -> Option<u64> {
        unsafe { self.godot.value(method, self.device, args, 0u64) }
    }

    fn compile(&mut self, plan: &EffectReadback) -> Option<bool> {
        let godot = self.godot;
        let source = godot.instantiate(c"RDShaderSource")?;
        let code = GString::new(godot, unsafe { CStr::from_ptr(plan.shader) });
        unsafe {
            godot.call(
                Bind::SHADER_SOURCE_SET_STAGE_SOURCE,
                source.object(),
                &[arg(&SHADER_STAGE_COMPUTE), code.ptr()],
            )
        }
        .then_some(())?;
        let (object, allow_cache) = (source.object(), 1u8);
        self.source = Some(source);
        self.spirv = unsafe {
            godot.reference(
                Bind::SHADER_COMPILE_SPIRV_FROM_SOURCE,
                self.device,
                &[arg(&object), arg(&allow_cache)],
            )
        };
        let spirv = self.spirv.as_ref()?.object();
        let error = unsafe {
            godot.string(
                Bind::SHADER_SPIRV_GET_STAGE_COMPILE_ERROR,
                spirv,
                &[arg(&SHADER_STAGE_COMPUTE)],
            )
        }?;
        if !error.is_empty() {
            return Some(false);
        }
        let name = GString::empty(godot);
        self.shader = self.rid(Bind::SHADER_CREATE_FROM_SPIRV, &[arg(&spirv), name.ptr()])?;
        Some(self.shader != 0)
    }

    fn uniform(&self, kind: i64, binding: i64, ids: &[u64]) -> Option<Ref> {
        let godot = self.godot;
        let uniform = godot.instantiate(c"RDUniform")?;
        let object = uniform.object();
        unsafe {
            godot
                .call(Bind::UNIFORM_SET_UNIFORM_TYPE, object, &[arg(&kind)])
                .then_some(())?;
            godot
                .call(Bind::UNIFORM_SET_BINDING, object, &[arg(&binding)])
                .then_some(())?;
            for id in ids {
                godot
                    .call(Bind::UNIFORM_ADD_ID, object, &[arg(id)])
                    .then_some(())?;
            }
        }
        Some(uniform)
    }

    fn prepare(&mut self, plan: &EffectReadback, valid: &mut [u32; 4]) -> Option<()> {
        let godot = self.godot;
        let constants = TypedArray::new(godot, c"RDPipelineSpecializationConstant");
        self.pipeline = self.rid(
            Bind::COMPUTE_PIPELINE_CREATE,
            &[arg(&self.shader), constants.ptr()],
        )?;
        let sampler_state = godot.instantiate(c"RDSamplerState")?;
        let state = sampler_state.object();
        self.sampler_state = Some(sampler_state);
        self.sampler = self.rid(Bind::SAMPLER_CREATE, &[arg(&state)])?;
        let (size, data, usage, creation) =
            (i64::from(plan.bytes), PackedBytes::new(godot), 0i64, 0i64);
        self.buffer = self.rid(
            Bind::STORAGE_BUFFER_CREATE,
            &[arg(&size), data.ptr(), arg(&usage), arg(&creation)],
        )?;
        self.texture_uniform = Some(self.uniform(
            UNIFORM_TYPE_SAMPLER_WITH_TEXTURE,
            0,
            &[self.sampler, self.motion],
        )?);
        self.buffer_uniform = Some(self.uniform(UNIFORM_TYPE_STORAGE_BUFFER, 1, &[self.buffer])?);
        let mut uniforms = TypedArray::new(godot, c"RDUniform");
        for uniform in [&self.texture_uniform, &self.buffer_uniform]
            .into_iter()
            .flatten()
        {
            uniforms.push_object(uniform.object());
        }
        let set = 0i64;
        self.uniform_set = self.rid(
            Bind::UNIFORM_SET_CREATE,
            &[uniforms.ptr(), arg(&self.shader), arg(&set)],
        )?;
        self.uniforms = Some(uniforms);
        *valid = [self.pipeline, self.sampler, self.buffer, self.uniform_set]
            .map(|rid| u32::from(rid != 0));
        Some(())
    }

    fn dispatch(&self, plan: &EffectReadback) -> Option<()> {
        let godot = self.godot;
        let list = unsafe { godot.value(Bind::COMPUTE_LIST_BEGIN, self.device, &[], 0i64) }?;
        let (set, x, y, z) = (
            0i64,
            i64::from(plan.groups_x),
            i64::from(plan.groups_y),
            1i64,
        );
        unsafe {
            godot
                .call(
                    Bind::COMPUTE_LIST_BIND_COMPUTE_PIPELINE,
                    self.device,
                    &[arg(&list), arg(&self.pipeline)],
                )
                .then_some(())?;
            godot
                .call(
                    Bind::COMPUTE_LIST_BIND_UNIFORM_SET,
                    self.device,
                    &[arg(&list), arg(&self.uniform_set), arg(&set)],
                )
                .then_some(())?;
            godot
                .call(
                    Bind::COMPUTE_LIST_DISPATCH,
                    self.device,
                    &[arg(&list), arg(&x), arg(&y), arg(&z)],
                )
                .then_some(())?;
            godot
                .call(Bind::COMPUTE_LIST_END, self.device, &[])
                .then_some(())
        }
    }

    fn read(&mut self) -> Option<()> {
        let (offset, size) = (0i64, 0i64);
        self.bytes = Some(unsafe {
            self.godot.bytes(
                Bind::BUFFER_GET_DATA,
                self.device,
                &[arg(&self.buffer), arg(&offset), arg(&size)],
            )
        }?);
        Some(())
    }

    fn free(&self, resource: u32) -> bool {
        let rid = match resource {
            RSF_GODOT_MOTION_SHADER => self.shader,
            RSF_GODOT_MOTION_PIPELINE => self.pipeline,
            RSF_GODOT_MOTION_SAMPLER => self.sampler,
            RSF_GODOT_MOTION_BUFFER => self.buffer,
            RSF_GODOT_MOTION_UNIFORMS => self.uniform_set,
            _ => return false,
        };
        rid == 0 || unsafe { self.godot.call(Bind::FREE_RID, self.device, &[arg(&rid)]) }
    }
}

fn readback<'a>(call: *mut c_void) -> &'a mut Readback {
    // SAFETY: `leaf_read_motion` lends its `Readback` for the synchronous sequence.
    unsafe { &mut *call.cast::<Readback>() }
}

unsafe extern "C" fn compile_motion(call: *mut c_void, plan: *const EffectReadback) -> u32 {
    (readback(call).compile(unsafe { &*plan }) == Some(true)).into()
}

unsafe extern "C" fn prepare_motion(
    call: *mut c_void,
    plan: *const EffectReadback,
    valid: *mut u32,
) -> u32 {
    let valid = unsafe { &mut *valid.cast::<[u32; 4]>() };
    readback(call)
        .prepare(unsafe { &*plan }, valid)
        .is_some()
        .into()
}

unsafe extern "C" fn dispatch_motion(call: *mut c_void, plan: *const EffectReadback) -> u32 {
    readback(call).dispatch(unsafe { &*plan }).is_some().into()
}

unsafe extern "C" fn read_motion(call: *mut c_void) -> u32 {
    readback(call).read().is_some().into()
}

unsafe extern "C" fn free_motion_resource(call: *mut c_void, resource: u32) -> u32 {
    readback(call).free(resource).into()
}
