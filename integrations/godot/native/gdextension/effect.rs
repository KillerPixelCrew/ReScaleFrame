// SPDX-License-Identifier: MIT
//! `ReScaleFrameEffect`: a `CompositorEffect` with the `POST_TRANSPARENT` callback, resolved
//! color and depth and motion vectors. Each callback publishes copied CPU facts of its scene
//! buffers and, when one was requested, a classified motion readback; scene objects, RIDs and
//! native pointers are borrowed only inside the callback, and no resource lease or GPU completion
//! is implied. The probe reads no Runtime settings or status: `ReScaleFrameRuntime` reports
//! active SR, FG and latency.
use super::api::{self, Bind, GString, Godot, PackedBytes, Ref, StringName, TypedArray, arg};
use super::sys::*;
use super::{Dictionary, EFFECT_CLASS, Sink};
use crate::effect::{Capture, classify};
use rsf_game_sys::ffi_util::lock_or_recover;
use std::ffi::{CStr, c_char, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// `CompositorEffect::EFFECT_CALLBACK_TYPE_POST_TRANSPARENT`.
const POST_TRANSPARENT: i64 = 4;
/// `_render_callback(int, RenderData)` in the pinned 4.7 API.
const RENDER_CALLBACK_HASH: u32 = 2153422729;
/// `RenderingDevice::DATA_FORMAT_R16G16_SFLOAT`, the only velocity format read back.
const DATA_FORMAT_R16G16_SFLOAT: i32 = 82;
const SHADER_STAGE_COMPUTE: i64 = 4;
const UNIFORM_TYPE_SAMPLER_WITH_TEXTURE: i64 = 1;
const UNIFORM_TYPE_STORAGE_BUFFER: i64 = 8;
const DRIVER_RESOURCE_LOGICAL_DEVICE: i64 = 0;
const DRIVER_RESOURCE_COMMAND_QUEUE: i64 = 3;
const DRIVER_RESOURCE_TEXTURE: i64 = 5;

const STAGE: &CStr = c"renderer_probe";
const REASON: &CStr = c"Compositor motion producer/probe only. Query ReScaleFrameRuntime for active SR, FG and latency.";
const SNAPSHOT_SCOPE: &CStr = c"last_post_transparent_callback";

// Convert the sampleable velocity texture into an owned readback buffer.
const MOTION_SHADER: &CStr = c"#version 450
layout(local_size_x=8, local_size_y=8) in;
layout(set=0,binding=0) uniform sampler2D motion_tex;
layout(set=0,binding=1,std430) writeonly buffer Samples { vec2 velocity[]; };
void main() {
    ivec2 p=ivec2(gl_GlobalInvocationID.xy), size=textureSize(motion_tex,0);
    if(any(greaterThanEqual(p,size))) return;
    velocity[p.y*size.x+p.x]=texelFetch(motion_tex,p,0).xy;
}";

unsafe extern "C" {
    /// The CMake project version (`runtime_platform.c`).
    fn rsf_godot_version() -> *const c_char;
}

/// The scene buffer facts of one callback; the godot-cpp defaults until they are read.
#[derive(Clone, Copy)]
struct Frame {
    render_target: u64,
    views: u32,
    internal: [i32; 2],
    output: [i32; 2],
    camera_transform: [f32; 12],
    jittered_projection: [f32; 16],
    color_valid: bool,
    depth_valid: bool,
    motion_valid: bool,
}

impl Default for Frame {
    fn default() -> Self {
        let mut camera_transform = [0.0; 12];
        let mut jittered_projection = [0.0; 16];
        for axis in 0..3 {
            camera_transform[axis * 3 + axis] = 1.0;
        }
        for axis in 0..4 {
            jittered_projection[axis * 4 + axis] = 1.0;
        }
        Self {
            render_target: 0,
            views: 0,
            internal: [0; 2],
            output: [0; 2],
            camera_transform,
            jittered_projection,
            color_valid: false,
            depth_valid: false,
            motion_valid: false,
        }
    }
}

/// The admitted buffers' formats and native handle facts; format -1 when not admitted.
#[derive(Clone, Copy)]
struct Resources {
    color_format: i32,
    depth_format: i32,
    motion_format: i32,
    native_device: bool,
    native_queue: bool,
    native_textures: bool,
}

impl Default for Resources {
    fn default() -> Self {
        Self {
            color_format: -1,
            depth_format: -1,
            motion_format: -1,
            native_device: false,
            native_queue: false,
            native_textures: false,
        }
    }
}

/// What the last callback published.
#[derive(Clone, Copy, Default)]
struct Snapshot {
    callbacks: u64,
    frame: Frame,
    resources: Resources,
    buffers_available: bool,
}

#[derive(Default)]
struct Published {
    snapshot: Snapshot,
    capture: Capture,
}

/// The engine owns the object; the instance is the probe it lends to each callback and method.
#[derive(Default)]
struct Instance {
    /// Set by `request_motion_capture` on any thread, taken by the next callback.
    requested: AtomicBool,
    published: Mutex<Published>,
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
    let instance = Box::into_raw(Box::new(Instance::default()));
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

/// `_render_callback(effect_callback_type, render_data)` on the render thread. The request is
/// taken first, so a callback that cannot read the motion still answers it with an unsucceeded
/// capture.
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
    let requested = instance.requested.swap(false, Ordering::SeqCst);
    let mut call = Call {
        godot,
        data,
        buffers: None,
        device: std::ptr::null_mut(),
        color: 0,
        depth: 0,
        motion: 0,
    };
    let mut next = Snapshot::default();
    let mut sampled = Capture::default();
    let mut frame = Frame::default();
    if call.read_frame(&mut frame).is_some() {
        next.frame = frame;
        next.buffers_available = frame.views == 1
            && frame.internal[0] > 0
            && frame.internal[1] > 0
            && frame.color_valid
            && frame.depth_valid
            && frame.motion_valid;
        if next.buffers_available {
            let mut resources = Resources::default();
            if call.read_resources(&mut resources).is_some() {
                next.resources = resources;
            }
            if requested && next.resources.motion_format == DATA_FORMAT_R16G16_SFLOAT {
                // Explicit bounded diagnostics only: the readback stalls the GPU.
                let [width, height] = frame.internal.map(i32::unsigned_abs);
                let pixels = u64::from(width) * u64::from(height);
                if let Some(bytes) = pixels
                    .checked_mul(8)
                    .and_then(|bytes| u32::try_from(bytes).ok())
                {
                    let mut readback = Readback::new(godot, call.device, call.motion);
                    let recorded = readback.record(bytes, width.div_ceil(8), height.div_ceil(8));
                    // The PackedByteArray owns the CPU copy until the classification.
                    let data = readback.bytes.take();
                    drop(readback);
                    let (bytes, count) = data
                        .as_ref()
                        .filter(|_| recorded)
                        .map_or((std::ptr::null(), 0), PackedBytes::data);
                    let bytes = if bytes.is_null() || count == 0 {
                        &[][..]
                    } else {
                        unsafe { std::slice::from_raw_parts(bytes, count as usize) }
                    };
                    sampled = classify(bytes, pixels);
                }
            }
        }
    }
    let mut published = lock_or_recover(&instance.published);
    next.callbacks = published.snapshot.callbacks.wrapping_add(1);
    if requested {
        sampled.count = published.capture.count.wrapping_add(1);
        sampled.callback = next.callbacks;
        published.capture = sampled;
    }
    published.snapshot = next;
}

/// `get_status()`.
///
/// # Safety
/// `instance` is a live instance from `create`.
pub(super) unsafe fn get_status(godot: &'static Godot, instance: *mut c_void) -> Dictionary {
    let instance = unsafe { &*instance.cast::<Instance>() };
    let (snapshot, capture) = {
        let published = lock_or_recover(&instance.published);
        (published.snapshot, published.capture)
    };
    let mut dictionary = Dictionary::new(godot);
    let mut sink = Sink {
        godot,
        rendering: godot.rendering_server(),
        dictionary: &mut dictionary,
    };
    sink.text(c"version", unsafe { CStr::from_ptr(rsf_godot_version()) });
    sink.text(c"stage", STAGE);
    // A probe: SR, FG and latency are the Runtime's to report.
    sink.boolean(c"rendering_ready", false);
    sink.boolean(c"sr_active", false);
    sink.boolean(c"fg_active", false);
    sink.boolean(c"latency_active", false);
    sink.text(c"reason", REASON);
    sink.name(c"renderer", Bind::GET_RENDERING_METHOD);
    sink.name(c"driver", Bind::GET_RENDERING_DRIVER_NAME);
    sink.text(c"snapshot_scope", SNAPSHOT_SCOPE);
    sink.integer(c"callbacks", snapshot.callbacks as i64);
    let frame = &snapshot.frame;
    sink.integer(c"render_target_id", frame.render_target as i64);
    sink.integer(c"view_count", frame.views.into());
    sink.extent(c"internal_size", frame.internal);
    sink.extent(c"output_size", frame.output);
    sink.transform(c"camera_transform", &frame.camera_transform);
    sink.projection(c"jittered_projection", &frame.jittered_projection);
    let resources = &snapshot.resources;
    sink.integer(c"color_format", resources.color_format.into());
    sink.integer(c"depth_format", resources.depth_format.into());
    sink.integer(c"motion_format", resources.motion_format.into());
    sink.boolean(c"buffers_available", snapshot.buffers_available);
    sink.boolean(c"native_device_available", resources.native_device);
    sink.boolean(c"native_queue_available", resources.native_queue);
    sink.boolean(c"native_textures_available", resources.native_textures);
    sink.integer(c"motion_captures", capture.count as i64);
    sink.integer(c"capture_callback", capture.callback as i64);
    sink.boolean(c"capture_succeeded", capture.succeeded);
    sink.integer(c"capture_pixels", capture.pixels as i64);
    sink.integer(c"capture_moving_pixels", capture.moving_pixels as i64);
    sink.integer(c"capture_nonfinite_pixels", capture.nonfinite_pixels as i64);
    sink.integer(c"capture_unwritten_pixels", capture.unwritten_pixels as i64);
    sink.real(c"capture_max_abs_uv", capture.max_abs_uv);
    dictionary
}

/// `request_motion_capture()`.
///
/// # Safety
/// `instance` is a live instance from `create`.
pub(super) unsafe fn request_motion_capture(instance: *mut c_void) {
    unsafe { &*instance.cast::<Instance>() }
        .requested
        .store(true, Ordering::SeqCst);
}

/// One `POST_TRANSPARENT` callback's borrowed scene state.
struct Call {
    godot: &'static Godot,
    data: ObjectPtr,
    buffers: Option<Ref>,
    device: ObjectPtr,
    color: u64,
    depth: u64,
    motion: u64,
}

impl Call {
    fn layer(&self, method: Bind, buffers: ObjectPtr) -> Option<u64> {
        let (index, msaa) = (0i64, 0u8);
        unsafe {
            self.godot
                .value(method, buffers, &[arg(&index), arg(&msaa)], 0u64)
        }
    }

    fn texture_valid(&self, texture: u64) -> Option<bool> {
        unsafe {
            self.godot
                .value(Bind::TEXTURE_IS_VALID, self.device, &[arg(&texture)], 0u8)
        }
        .map(|valid| valid != 0)
    }

    /// The frame facts; `None` without a RenderingDevice, scene buffers or scene data, or when an
    /// engine read failed.
    fn read_frame(&mut self, frame: &mut Frame) -> Option<()> {
        let godot = self.godot;
        // Only RenderSceneBuffersRD converts to the Ref the scene reads use.
        self.buffers = unsafe { godot.reference(Bind::GET_RENDER_SCENE_BUFFERS, self.data, &[]) }
            .filter(|buffers| godot.is_class(buffers.object(), c"RenderSceneBuffersRD"));
        let null = std::ptr::null_mut::<c_void>();
        let scene = unsafe { godot.value(Bind::GET_RENDER_SCENE_DATA, self.data, &[], null) }?;
        let rendering = godot.rendering_server();
        let device = if rendering.is_null() {
            null
        } else {
            unsafe { godot.value(Bind::GET_RENDERING_DEVICE, rendering, &[], null) }?
        };
        let buffers = self.buffers.as_ref().map(Ref::object);
        let (Some(buffers), false, false) = (buffers, device.is_null(), scene.is_null()) else {
            return None;
        };
        self.device = device;
        frame.render_target = unsafe { godot.value(Bind::GET_RENDER_TARGET, buffers, &[], 0u64) }?;
        frame.views = unsafe { godot.value(Bind::GET_VIEW_COUNT, buffers, &[], 0i64) }? as u32;
        frame.internal = unsafe { godot.value(Bind::GET_INTERNAL_SIZE, buffers, &[], [0i32; 2]) }?;
        frame.output = unsafe { godot.value(Bind::GET_TARGET_SIZE, buffers, &[], [0i32; 2]) }?;
        // Transform3D is basis rows then origin, Projection is columns: both copy in memory order.
        frame.camera_transform =
            unsafe { godot.value(Bind::GET_CAM_TRANSFORM, scene, &[], [0f32; 12]) }?;
        frame.jittered_projection =
            unsafe { godot.value(Bind::GET_CAM_PROJECTION, scene, &[], [0f32; 16]) }?;
        self.color = self.layer(Bind::GET_COLOR_LAYER, buffers)?;
        self.depth = self.layer(Bind::GET_DEPTH_LAYER, buffers)?;
        self.motion = self.layer(Bind::GET_VELOCITY_LAYER, buffers)?;
        frame.color_valid = self.texture_valid(self.color)?;
        frame.depth_valid = self.texture_valid(self.depth)?;
        frame.motion_valid = self.texture_valid(self.motion)?;
        Some(())
    }

    /// The formats and native handle facts, or `None` when an engine read failed.
    fn read_resources(&self, resources: &mut Resources) -> Option<()> {
        let godot = self.godot;
        let format = |texture: u64| -> Option<i32> {
            let format = unsafe {
                godot.reference(Bind::TEXTURE_GET_FORMAT, self.device, &[arg(&texture)])
            }?;
            unsafe { godot.value(Bind::TEXTURE_FORMAT_GET_FORMAT, format.object(), &[], 0i64) }
                .map(|value| value as i32)
        };
        let driver = |resource: i64, rid: u64| -> Option<bool> {
            let index = 0u64;
            unsafe {
                godot.value(
                    Bind::GET_DRIVER_RESOURCE,
                    self.device,
                    &[arg(&resource), arg(&rid), arg(&index)],
                    0u64,
                )
            }
            .map(|handle| handle != 0)
        };
        resources.color_format = format(self.color)?;
        resources.depth_format = format(self.depth)?;
        resources.motion_format = format(self.motion)?;
        resources.native_device = driver(DRIVER_RESOURCE_LOGICAL_DEVICE, 0)?;
        resources.native_queue = driver(DRIVER_RESOURCE_COMMAND_QUEUE, 0)?;
        resources.native_textures = driver(DRIVER_RESOURCE_TEXTURE, self.color)?
            && driver(DRIVER_RESOURCE_TEXTURE, self.depth)?
            && driver(DRIVER_RESOURCE_TEXTURE, self.motion)?;
        Some(())
    }
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
    // Dropped in this order, the reverse of the godot-cpp locals.
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

    fn compile(&mut self) -> Option<bool> {
        let godot = self.godot;
        let source = godot.instantiate(c"RDShaderSource")?;
        let code = GString::new(godot, MOTION_SHADER);
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

    fn prepare(&mut self, bytes: u32) -> Option<()> {
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
        let (size, data, usage, creation) = (i64::from(bytes), PackedBytes::new(godot), 0i64, 0i64);
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
        Some(())
    }

    fn dispatch(&self, groups_x: u32, groups_y: u32) -> Option<()> {
        let godot = self.godot;
        let list = unsafe { godot.value(Bind::COMPUTE_LIST_BEGIN, self.device, &[], 0i64) }?;
        let (set, x, y, z) = (0i64, i64::from(groups_x), i64::from(groups_y), 1i64);
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

    /// Releases every RID, children first, and reports whether each release succeeded.
    fn free(&self) -> bool {
        let mut freed = true;
        for rid in [
            self.uniform_set,
            self.buffer,
            self.sampler,
            self.pipeline,
            self.shader,
        ] {
            freed &=
                rid == 0 || unsafe { self.godot.call(Bind::FREE_RID, self.device, &[arg(&rid)]) };
        }
        freed
    }

    /// Compiles, records, dispatches and reads the conversion of `bytes` bytes, then releases the
    /// resources created up to that point, even after a refusal.
    fn record(&mut self, bytes: u32, groups_x: u32, groups_y: u32) -> bool {
        let recorded = self.compile() == Some(true)
            && self.prepare(bytes).is_some()
            && ![self.pipeline, self.sampler, self.buffer, self.uniform_set].contains(&0)
            && self.dispatch(groups_x, groups_y).is_some()
            && self.read().is_some();
        self.free() && recorded
    }
}
