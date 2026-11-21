// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::overlay_abi::rsf_overlay_texture_update as Update;
use crate::overlay_geometry::{
    TEXTURE_EDGE, borrowed_draw_slice, patch_fits, valid_indices, valid_texture,
};
use crate::overlay_host_abi::{
    rsf_overlay_draw_call as DrawCall, rsf_overlay_draw_data as DrawData,
    rsf_overlay_vertex as Vertex,
};
use crate::render_abi::rsf_backend_log_fn as LogFn;
use crate::sr_session::has_capacity;
use std::ffi::{CString, c_char, c_void};
use std::mem::{self, offset_of};
use std::ptr::null_mut;
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

type Handle = *mut c_void;
const ABI: u32 = 1;
const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const RESOURCE_FAILED: i32 = -4;
const MAX_VERTICES: u32 = 1 << 22;
const MAX_INDICES: u32 = 1 << 23;
const GROWTH: u32 = 4096;
const VERTEX_BUFFER: u32 = 1;
const INDEX_BUFFER: u32 = 2;
const CONSTANTS: usize = 3;
const CLASS_INSTANCES: usize = 253; // D3D11_SHADER_MAX_INTERFACES
const VIEWPORTS: usize = 16; // D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE

#[repr(C)]
#[derive(Clone, Copy)]
struct Setup {
    struct_size: u32,
    abi_version: u32,
    log: LogFn,
    log_user: Handle,
}

// Same order as rsf_overlay11_pipeline. The leaf transfers any references
// it acquired, including on a partial creation failure.
type RawPipeline = [Handle; 8];

#[repr(C)]
struct Draw {
    view: Handle,
    index_count: u32,
    index_offset: u32,
    vertex_offset: i32,
    clip: [i32; 4],
}

// Mirrors rsf_overlay11_snapshot. The native leaf fills nullable COM pointers with owned
// references. Field order keeps their release order after the bindings are restored.
#[repr(C)]
struct Snapshot {
    predicate: Option<ComRef>,
    predicate_value: i32,
    input_layout: Option<ComRef>,
    topology: u32,
    vertex_buffer: Option<ComRef>,
    vertex_stride: u32,
    vertex_offset: u32,
    index_buffer: Option<ComRef>,
    index_format: u32,
    index_offset: u32,

    vertex_shader: Option<ComRef>,
    vertex_instances: [Option<ComRef>; CLASS_INSTANCES],
    vertex_instance_count: u32,
    vertex_constants: Option<ComRef>,

    pixel_shader: Option<ComRef>,
    pixel_instances: [Option<ComRef>; CLASS_INSTANCES],
    pixel_instance_count: u32,
    pixel_resource: Option<ComRef>,
    pixel_sampler: Option<ComRef>,

    geometry_shader: Option<ComRef>,
    geometry_instances: [Option<ComRef>; CLASS_INSTANCES],
    geometry_instance_count: u32,
    hull_shader: Option<ComRef>,
    hull_instances: [Option<ComRef>; CLASS_INSTANCES],
    hull_instance_count: u32,
    domain_shader: Option<ComRef>,
    domain_instances: [Option<ComRef>; CLASS_INSTANCES],
    domain_instance_count: u32,

    rasterizer: Option<ComRef>,
    viewports: [[f32; 6]; VIEWPORTS],
    viewport_count: u32,
    scissors: [[i32; 4]; VIEWPORTS],
    scissor_count: u32,

    blend: Option<ComRef>,
    blend_factor: [f32; 4],
    sample_mask: u32,
    depth_stencil: Option<ComRef>,
    stencil_reference: u32,
}

#[repr(C)]
struct Platform {
    pipeline: unsafe extern "C" fn(Handle, &Setup, &mut RawPipeline) -> i32,
    context: unsafe extern "C" fn(Handle, Handle) -> i32,
    texture: unsafe extern "C" fn(Handle, &Update, u32) -> Handle,
    view: unsafe extern "C" fn(Handle, Handle) -> Handle,
    patch: unsafe extern "C" fn(Handle, Handle, &Update, u32),
    buffer: unsafe extern "C" fn(Handle, u32, u32) -> Handle,
    write: unsafe extern "C" fn(Handle, Handle, *const c_void, usize) -> i32,
    target: unsafe extern "C" fn(Handle) -> u32,
    read_state: unsafe extern "C" fn(Handle, &mut Snapshot) -> i32,
    draw: unsafe extern "C" fn(
        Handle,
        &RawPipeline,
        Handle,
        Handle,
        u32,
        u32,
        *const Draw,
        usize,
    ) -> i32,
    restore_state: unsafe extern "C" fn(Handle, &Snapshot),
    log: unsafe extern "C" fn(LogFn, Handle, *const c_char),
}

const _: () = {
    assert!(size_of::<Setup>() == 24 && size_of::<Draw>() == 40);
    assert!(size_of::<Platform>() == 12 * size_of::<Handle>());
    assert!(size_of::<Snapshot>() == 10992 && align_of::<Snapshot>() == 8);
    assert!(offset_of!(Snapshot, vertex_instances) == 72);
    assert!(offset_of!(Snapshot, pixel_instances) == 2120);
    assert!(offset_of!(Snapshot, geometry_instances) == 4176);
    assert!(offset_of!(Snapshot, hull_instances) == 6216);
    assert!(offset_of!(Snapshot, domain_instances) == 8256);
    assert!(offset_of!(Snapshot, viewports) == 10296);
    assert!(offset_of!(Snapshot, scissors) == 10684);
    assert!(offset_of!(Snapshot, blend) == 10944);
};

struct Texture {
    id: u64,
    width: u32,
    height: u32,
    view: ComRef,
    resource: ComRef,
}

struct Buffer {
    resource: ComRef,
    capacity: u32,
}

#[derive(Default)]
struct State {
    textures: Vec<Texture>,
    vertices: Option<Buffer>,
    indices: Option<Buffer>,
    reported_missing: bool,
    checked_target: bool,
    reported_no_target: bool,
}

struct Renderer {
    state: Mutex<State>,
    pending_free: Mutex<Vec<u64>>,
    pipeline: [ComRef; 8],
    device: ComRef,
    setup: Setup,
    platform: &'static Platform,
}

impl Renderer {
    fn say(&self, text: &str) {
        if self.setup.log.is_some()
            && let Ok(text) = CString::new(text)
        {
            unsafe { (self.platform.log)(self.setup.log, self.setup.log_user, text.as_ptr()) };
        }
    }

    fn state(&self) -> Option<MutexGuard<'_, State>> {
        let mut state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return None,
        };
        let frees = mem::take(
            &mut *self
                .pending_free
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        for id in frees {
            state.free(id);
        }
        Some(state)
    }

    fn context_valid(&self, context: Handle) -> bool {
        !context.is_null() && unsafe { (self.platform.context)(self.device.as_ptr(), context) } != 0
    }

    fn upload(&self, context: Handle, update: &Update) -> (i32, Option<String>) {
        let Some(mut state) = self.state() else {
            return (RESOURCE_FAILED, None);
        };
        let position = state
            .textures
            .iter()
            .position(|texture| texture.id == update.id);
        let pitch = update.width * 4;
        if update.is_whole_texture != 0 {
            let device = self.device.as_ptr();
            let texture =
                unsafe { ComRef::from_owned((self.platform.texture)(device, update, pitch)) };
            let Some(resource) = texture else {
                return (
                    RESOURCE_FAILED,
                    Some(format!(
                        "overlay texture {} could not be created at {}x{}",
                        update.id, update.width, update.height
                    )),
                );
            };
            let view =
                unsafe { ComRef::from_owned((self.platform.view)(device, resource.as_ptr())) };
            let Some(view) = view else {
                return (
                    RESOURCE_FAILED,
                    Some(format!(
                        "overlay texture {} has no shader resource view",
                        update.id
                    )),
                );
            };
            let texture = Texture {
                id: update.id,
                width: update.width,
                height: update.height,
                view,
                resource,
            };
            if let Some(index) = position {
                state.textures[index] = texture;
            } else {
                if state.textures.try_reserve(1).is_err() {
                    return (RESOURCE_FAILED, None);
                }
                state.textures.push(texture);
            }
        } else {
            let Some(texture) = position.map(|index| &state.textures[index]) else {
                return (
                    INVALID,
                    Some(format!(
                        "overlay patch for texture {}, which was never uploaded whole",
                        update.id
                    )),
                );
            };
            if !patch_fits(update, texture.width, texture.height) {
                return (
                    INVALID,
                    Some(format!(
                        "overlay patch {}x{} at {},{} does not fit texture {} of {}x{}",
                        update.width,
                        update.height,
                        update.x,
                        update.y,
                        update.id,
                        texture.width,
                        texture.height
                    )),
                );
            }
            unsafe { (self.platform.patch)(context, texture.resource.as_ptr(), update, pitch) };
        }
        (OK, None)
    }

    fn grow(
        &self,
        current: &Option<Buffer>,
        count: u32,
        stride: u32,
        bind: u32,
    ) -> Result<Option<Buffer>, ()> {
        if current
            .as_ref()
            .is_some_and(|buffer| buffer.capacity >= count)
        {
            return Ok(None);
        }
        let capacity = count.div_ceil(GROWTH) * GROWTH;
        let resource = unsafe {
            ComRef::from_owned((self.platform.buffer)(
                self.device.as_ptr(),
                capacity * stride,
                bind,
            ))
        }
        .ok_or(())?;
        Ok(Some(Buffer { resource, capacity }))
    }

    fn draw(
        &self,
        context: Handle,
        data: &DrawData,
        indices: &[u32],
        calls: &[DrawCall],
        width: u32,
        height: u32,
    ) -> i32 {
        let mut notes = DrawNotes::default();
        let status = match self.state() {
            Some(mut state) => self.draw_locked(
                &mut state, &mut notes, context, data, indices, calls, width, height,
            ),
            None => RESOURCE_FAILED,
        };
        if notes.growth_failed {
            self.say(&format!(
                "overlay geometry buffers could not be grown to {} vertices and {} indices",
                data.vertex_count, data.index_count
            ));
        }
        if let Some(index) = notes.invalid_call {
            self.say(&format!(
                "overlay draw call {index} indexes outside the frame's own geometry"
            ));
        }
        if let Some(id) = notes.missing_texture {
            self.say(&format!(
                "overlay draw call names texture {id}, which was never uploaded"
            ));
        }
        if let Some(message) = notes.target {
            self.say(message);
        }
        status
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_locked(
        &self,
        state: &mut State,
        notes: &mut DrawNotes,
        context: Handle,
        data: &DrawData,
        indices: &[u32],
        calls: &[DrawCall],
        width: u32,
        height: u32,
    ) -> i32 {
        let mut plan = Vec::new();
        if plan.try_reserve_exact(calls.len()).is_err() {
            return RESOURCE_FAILED;
        }
        for (index, call) in calls.iter().enumerate() {
            if call.index_count == 0 {
                continue;
            }
            if !valid_indices(call, indices, data.vertex_count) {
                notes.invalid_call.get_or_insert(index);
                continue;
            }
            let Some(clip) = clip_rect(call, width, height) else {
                continue;
            };
            let Some(texture) = state
                .textures
                .iter()
                .find(|texture| texture.id == call.texture_id)
            else {
                if !state.reported_missing {
                    state.reported_missing = true;
                    notes.missing_texture = Some(call.texture_id);
                }
                continue;
            };
            plan.push(Draw {
                view: texture.view.as_ptr(),
                index_count: call.index_count,
                index_offset: call.index_offset,
                vertex_offset: call.vertex_offset as i32,
                clip,
            });
        }
        if plan.is_empty() {
            return OK;
        }
        let vertex_size = size_of::<Vertex>() as u32;
        let index_size = size_of::<u32>() as u32;
        let (Ok(vertices), Ok(indices)) = (
            self.grow(
                &state.vertices,
                data.vertex_count,
                vertex_size,
                VERTEX_BUFFER,
            ),
            self.grow(&state.indices, data.index_count, index_size, INDEX_BUFFER),
        ) else {
            notes.growth_failed = true;
            return RESOURCE_FAILED;
        };
        if let Some(vertices) = vertices {
            state.vertices = Some(vertices);
        }
        if let Some(indices) = indices {
            state.indices = Some(indices);
        }
        let vertices = state.vertices.as_ref().unwrap().resource.as_ptr();
        let indices = state.indices.as_ref().unwrap().resource.as_ptr();
        let constants = [2.0f32 / width as f32, 2.0f32 / height as f32, 0.0, 0.0];
        let writes = [
            (
                vertices,
                data.vertices.cast(),
                (data.vertex_count * vertex_size) as usize,
            ),
            (
                indices,
                data.indices.cast(),
                (data.index_count * index_size) as usize,
            ),
            (
                self.pipeline[CONSTANTS].as_ptr(),
                constants.as_ptr().cast(),
                size_of_val(&constants),
            ),
        ];
        if writes.iter().any(|&(buffer, bytes, length)| unsafe {
            (self.platform.write)(context, buffer, bytes, length) == 0
        }) {
            return RESOURCE_FAILED;
        }
        if !state.checked_target {
            match unsafe { (self.platform.target)(context) } {
                0 if !state.reported_no_target => {
                    state.reported_no_target = true;
                    notes.target = Some(
                        "no render target was bound when the overlay drew, so it went nowhere",
                    );
                }
                2 => {
                    state.checked_target = true;
                    notes.target = Some(
                        "the bound render target is an sRGB view, so the overlay's colours are encoded twice and will look washed out",
                    );
                }
                0 => {}
                _ => state.checked_target = true,
            }
        }
        let pipeline = std::array::from_fn(|i| self.pipeline[i].as_ptr());
        // Zero is valid for each numeric field and is None for each nullable COM pointer.
        let mut snapshot: Snapshot = unsafe { mem::zeroed() };
        if unsafe { (self.platform.read_state)(context, &mut snapshot) } == 0 {
            return RESOURCE_FAILED;
        }
        let drawn = unsafe {
            let drawn = (self.platform.draw)(
                context,
                &pipeline,
                vertices,
                indices,
                width,
                height,
                plan.as_ptr(),
                plan.len(),
            );
            (self.platform.restore_state)(context, &snapshot);
            drawn
        };
        drop(snapshot);
        if drawn != 0 { OK } else { RESOURCE_FAILED }
    }
}

impl State {
    fn free(&mut self, id: u64) {
        if let Some(index) = self.textures.iter().position(|texture| texture.id == id) {
            self.textures.swap_remove(index);
        }
    }
}

#[derive(Default)]
struct DrawNotes {
    growth_failed: bool,
    invalid_call: Option<usize>,
    missing_texture: Option<u64>,
    target: Option<&'static str>,
}

fn clip_rect(call: &DrawCall, width: u32, height: u32) -> Option<[i32; 4]> {
    let right = call.clip_x.saturating_add(call.clip_width).min(width);
    let bottom = call.clip_y.saturating_add(call.clip_height).min(height);
    if call.clip_x >= right || call.clip_y >= bottom {
        return None;
    }
    Some([
        i32::try_from(call.clip_x).ok()?,
        i32::try_from(call.clip_y).ok()?,
        i32::try_from(right).ok()?,
        i32::try_from(bottom).ok()?,
    ])
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay11_create(
    device: Handle,
    setup: *const Setup,
    out: *mut *mut Renderer,
    platform: &'static Platform,
) -> i32 {
    if device.is_null() || out.is_null() || !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != ABI {
        return ABI_MISMATCH;
    }
    unsafe { out.write(null_mut()) };
    let mut raw = [null_mut(); 8];
    let status = unsafe { (platform.pipeline)(device, setup, &mut raw) };
    let pipeline = raw.map(|handle| unsafe { ComRef::from_owned(handle) });
    if status != OK {
        return status;
    }
    let [
        Some(vs),
        Some(ps),
        Some(layout),
        Some(constants),
        Some(blend),
        Some(depth),
        Some(raster),
        Some(sampler),
    ] = pipeline
    else {
        return RESOURCE_FAILED;
    };
    let renderer = crate::allocate_handle(Renderer {
        state: Mutex::new(State::default()),
        pending_free: Mutex::new(Vec::new()),
        pipeline: [vs, ps, layout, constants, blend, depth, raster, sampler],
        device: unsafe { ComRef::retain(device) },
        setup: *setup,
        platform,
    });
    let Some(owner) = (unsafe { renderer.as_ref() }) else {
        return RESOURCE_FAILED;
    };
    owner.say("overlay renderer ready");
    unsafe { out.write(renderer) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay11_upload(
    renderer: *const Renderer,
    context: Handle,
    update: *const Update,
) -> i32 {
    let (Some(renderer), Some(update)) = (unsafe { renderer.as_ref() }, unsafe { update.as_ref() })
    else {
        return INVALID;
    };
    if !valid_texture(update) {
        if update.width > TEXTURE_EDGE || update.height > TEXTURE_EDGE {
            renderer.say(&format!(
                "overlay texture update of {}x{} is larger than D3D11 allows",
                update.width, update.height
            ));
        }
        return INVALID;
    }
    if !renderer.context_valid(context) {
        return INVALID;
    }
    let (status, note) = renderer.upload(context, update);
    if let Some(note) = note {
        renderer.say(&note);
    }
    status
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay11_free(renderer: *const Renderer, id: u64) {
    if let Some(renderer) = unsafe { renderer.as_ref() } {
        if let Some(mut state) = renderer.state() {
            state.free(id);
        } else {
            renderer
                .pending_free
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(id);
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay11_draw(
    renderer: *const Renderer,
    context: Handle,
    data: *const DrawData,
    width: u32,
    height: u32,
) -> i32 {
    let Some(renderer) = (unsafe { renderer.as_ref() }) else {
        return INVALID;
    };
    if context.is_null() || width == 0 || height == 0 || !unsafe { has_capacity(data) } {
        return INVALID;
    }
    let data = unsafe { &*data };
    if data.call_count == 0 || data.vertex_count == 0 || data.index_count == 0 {
        return OK;
    }
    if data.vertex_count > MAX_VERTICES || data.index_count > MAX_INDICES {
        renderer.say(&format!(
            "overlay frame of {} vertices and {} indices is past any plausible ceiling",
            data.vertex_count, data.index_count
        ));
        return INVALID;
    }
    if data.vertices.is_null() || !data.vertices.is_aligned() || !renderer.context_valid(context) {
        return INVALID;
    }
    let Some(indices) = (unsafe { borrowed_draw_slice(data.indices, data.index_count) }) else {
        return INVALID;
    };
    let Some(calls) = (unsafe { borrowed_draw_slice(data.calls, data.call_count) }) else {
        return INVALID;
    };
    renderer.draw(context, data, indices, calls, width, height)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay11_destroy(renderer: *mut Renderer) {
    if !renderer.is_null() {
        // C clients retain ownership of the handle through each operation.
        drop(unsafe { Box::from_raw(renderer) });
    }
}
