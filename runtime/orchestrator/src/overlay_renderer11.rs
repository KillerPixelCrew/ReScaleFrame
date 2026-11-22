// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::overlay_abi::rsf_overlay_texture_update as Update;
use crate::overlay_geometry::{
    TEXTURE_EDGE, borrowed_draw_slice, draw_clip, patch_fits, valid_indices, valid_texture,
};
use crate::overlay_host_abi::{
    rsf_overlay_draw_call as DrawCall, rsf_overlay_draw_data as DrawData,
    rsf_overlay_vertex as Vertex,
};
use crate::render_abi::rsf_backend_log_fn as LogFn;
use crate::sr_session::has_capacity;
use std::ffi::{CString, c_char, c_void};
use std::mem;
use std::ptr::null_mut;
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

mod pipeline;
mod platform;

type Handle = *mut c_void;
const ABI: u32 = 1;
const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const SHADER_FAILED: i32 = -3;
const RESOURCE_FAILED: i32 = -4;
const MAX_VERTICES: u32 = 1 << 22;
const MAX_INDICES: u32 = 1 << 23;
const GROWTH: u32 = 4096;
const VERTEX_BUFFER: u32 = 1;
const INDEX_BUFFER: u32 = 2;
const CONSTANTS: usize = 3;

#[repr(C)]
#[derive(Clone, Copy)]
struct Setup {
    struct_size: u32,
    abi_version: u32,
    log: LogFn,
    log_user: Handle,
}

type RawPipeline = [Handle; 8];

struct Draw {
    view: Handle,
    index_count: u32,
    index_offset: u32,
    vertex_offset: i32,
    clip: [i32; 4],
}

unsafe extern "C" {
    fn rsf_overlay_renderer_log(log: LogFn, user: Handle, message: *const c_char);
}

impl Setup {
    fn say(&self, text: &str) {
        if self.log.is_some()
            && let Ok(text) = CString::new(text)
        {
            unsafe { rsf_overlay_renderer_log(self.log, self.log_user, text.as_ptr()) };
        }
    }
}

const _: () = assert!(size_of::<Setup>() == 24);

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
}

impl Renderer {
    fn say(&self, text: &str) {
        self.setup.say(text);
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
        unsafe { platform::valid_context(self.device.as_ptr(), context) }
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
            let texture = unsafe { platform::texture(device, update, pitch) };
            let Some(resource) = texture else {
                return (
                    RESOURCE_FAILED,
                    Some(format!(
                        "overlay texture {} could not be created at {}x{}",
                        update.id, update.width, update.height
                    )),
                );
            };
            let view = unsafe { platform::view(device, resource.as_ptr()) };
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
            unsafe { platform::patch(context, texture.resource.as_ptr(), update, pitch) };
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
        let resource =
            unsafe { platform::buffer(self.device.as_ptr(), capacity * stride, bind) }.ok_or(())?;
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
            let Ok(Some(clip)) = draw_clip(call, width, height) else {
                continue;
            };
            if !valid_indices(call, indices, data.vertex_count) {
                notes.invalid_call.get_or_insert(index);
                continue;
            }
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
            !platform::write(context, buffer, bytes, length)
        }) {
            return RESOURCE_FAILED;
        }
        if !state.checked_target {
            match unsafe { platform::target_kind(context) } {
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
        unsafe {
            platform::draw(
                context,
                &pipeline,
                vertices,
                indices,
                [width, height],
                &plan,
            );
        }
        OK
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_overlay_renderer_create(
    device: Handle,
    setup: *const Setup,
    out: *mut Handle,
) -> i32 {
    if device.is_null() || out.is_null() || !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != ABI {
        return ABI_MISMATCH;
    }
    unsafe { out.write(null_mut()) };
    let pipeline = match unsafe { pipeline::create(device, setup) } {
        Ok(pipeline) => pipeline,
        Err(status) => return status,
    };
    let renderer = crate::allocate_handle(Renderer {
        state: Mutex::new(State::default()),
        pending_free: Mutex::new(Vec::new()),
        pipeline,
        device: unsafe { ComRef::retain(device) },
        setup: *setup,
    });
    let Some(owner) = (unsafe { renderer.as_ref() }) else {
        return RESOURCE_FAILED;
    };
    owner.say("overlay renderer ready");
    unsafe { out.write(renderer.cast()) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_overlay_renderer_upload_texture(
    renderer: Handle,
    context: Handle,
    update: *const Update,
) -> i32 {
    let (Some(renderer), Some(update)) =
        (unsafe { renderer.cast::<Renderer>().as_ref() }, unsafe {
            update.as_ref()
        })
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
unsafe extern "C" fn rsf_overlay_renderer_free_texture(renderer: Handle, id: u64) {
    if let Some(renderer) = unsafe { renderer.cast::<Renderer>().as_ref() } {
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
unsafe extern "C" fn rsf_overlay_renderer_draw(
    renderer: Handle,
    context: Handle,
    data: *const DrawData,
    width: u32,
    height: u32,
) -> i32 {
    let Some(renderer) = (unsafe { renderer.cast::<Renderer>().as_ref() }) else {
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
unsafe extern "C" fn rsf_overlay_renderer_destroy(renderer: Handle) {
    if !renderer.is_null() {
        // C clients retain ownership of the handle through each operation.
        drop(unsafe { Box::from_raw(renderer.cast::<Renderer>()) });
    }
}
