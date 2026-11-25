// SPDX-License-Identifier: MIT
use crate::com_ref;
use crate::d3d11_output::{GraphicsCache, texture_desc};
use crate::d3d11_output_platform::{
    rsf_d3d11_output_child_device, rsf_d3d11_output_copy_resource, rsf_d3d11_output_create_texture,
    rsf_d3d11_output_query_texture,
};
use crate::sr_session::has_capacity;
use crate::sys::{
    RSF_D3D11_BIND_SHADER_RESOURCE, RSF_D3D11_USAGE_DEFAULT, rsf_native_composition_frame as Frame,
};
use rsf_game_sys::{
    RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_UI_COMPOSITE, rsf_game_render_pass as Pass,
};
use std::ffi::c_void;
use std::ptr;
use std::sync::Mutex;
use windows::core::{IUnknown, Interface};

struct Request {
    remaining: u32,
    epoch: u64,
}

impl Request {
    fn take_scope(&mut self) -> Option<u64> {
        if self.remaining == 0 {
            return None;
        }
        if self.remaining != u32::MAX {
            self.remaining -= 1;
        }
        Some(self.epoch)
    }
}

// Diagnostic callers can arm the next capture while a copy is being recorded.
// A cancellation invalidates both pending and completed scopes immediately.
static REQUEST: Mutex<Request> = Mutex::new(Request {
    remaining: 0,
    epoch: 0,
});

#[derive(Clone, Copy, PartialEq, Eq)]
struct ScopeKey {
    stream: usize,
    epoch: u64,
    session_id: u64,
    family_key: u64,
    view_key: u64,
    native_frame: u64,
    scope_id: u64,
    source_frame_id: u64,
    submission_id: u64,
    viewport_key: u64,
    output_rect: [i32; 4],
}

impl ScopeKey {
    fn new(context: *mut c_void, pass: &Pass, epoch: u64) -> Self {
        Self {
            stream: context as usize,
            epoch,
            session_id: pass.session_id,
            family_key: pass.family_key,
            view_key: pass.view_key,
            native_frame: pass.native_frame,
            scope_id: pass.scope_id,
            source_frame_id: pass.source_frame_id,
            submission_id: pass.submission_id,
            viewport_key: pass.viewport_key,
            output_rect: pass.output_rect,
        }
    }
}

#[derive(Clone, Copy)]
struct Scope {
    key: ScopeKey,
    flags: u32,
    has_ui: bool,
}

enum Capture {
    Idle,
    Pending(Scope),
    Complete(Scope),
}

impl Capture {
    fn take_pending(&mut self, key: ScopeKey) -> Option<Scope> {
        let Self::Pending(scope) = *self else {
            return None;
        };
        if scope.key != key {
            return None;
        }
        *self = Self::Idle;
        Some(scope)
    }
}

struct Image {
    texture: IUnknown,
    device: IUnknown,
    width: u32,
    height: u32,
    format: u32,
}

// SAFETY: only D3D11 device and texture references are stored. Teardown may
// retire them on another thread after the graphics execution owner drains.
unsafe impl Send for Image {}

impl Image {
    unsafe fn capture(
        slot: &mut Option<Self>,
        context: *mut c_void,
        source: *mut c_void,
        generation: &mut u32,
    ) -> Option<()> {
        if source.is_null() {
            return None;
        }
        let input = unsafe { com_ref::owned(rsf_d3d11_output_query_texture(source)) }?;
        let mut desc = unsafe { texture_desc(input.as_raw()) };
        if desc.samples != 1 || desc.layers != 1 || desc.mips != 1 {
            return None;
        }
        let device = unsafe { com_ref::owned(rsf_d3d11_output_child_device(context)) }?;
        let input_device =
            unsafe { com_ref::owned(rsf_d3d11_output_child_device(input.as_raw())) }?;
        if input_device.as_raw() != device.as_raw() {
            return None;
        }
        let snapshot = if let Some(image) = slot.as_ref().filter(|image| {
            image.device.as_raw() == device.as_raw()
                && image.width == desc.width
                && image.height == desc.height
                && image.format == desc.format
        }) {
            image
        } else {
            desc.usage = RSF_D3D11_USAGE_DEFAULT;
            desc.cpu_access_flags = 0;
            desc.misc_flags = 0;
            desc.bind_flags = RSF_D3D11_BIND_SHADER_RESOURCE;
            let texture =
                unsafe { com_ref::owned(rsf_d3d11_output_create_texture(device.as_raw(), &desc)) }?;
            *generation = generation.wrapping_add(1);
            slot.insert(Self {
                texture,
                device,
                width: desc.width,
                height: desc.height,
                format: desc.format,
            })
        };
        unsafe {
            rsf_d3d11_output_copy_resource(context, snapshot.texture.as_raw(), input.as_raw())
        };
        Some(())
    }
}

struct Composition {
    scene: Option<Image>,
    ui: Option<Image>,
    composed: Option<Image>,
    capture: Capture,
    generation: u32,
}

impl Composition {
    const fn new() -> Self {
        Self {
            scene: None,
            ui: None,
            composed: None,
            capture: Capture::Idle,
            generation: 0,
        }
    }

    unsafe fn capture(&mut self, context: *mut c_void, pass: &Pass, begin: bool) -> Option<()> {
        if begin {
            let epoch = REQUEST.lock().ok()?.take_scope()?;
            self.capture = Capture::Idle;
            let scope = Scope {
                key: ScopeKey::new(context, pass, epoch),
                flags: pass.flags,
                has_ui: !pass.ui_input.is_null(),
            };
            unsafe {
                Image::capture(
                    &mut self.scene,
                    context,
                    pass.color_input,
                    &mut self.generation,
                )
            }?;
            if scope.has_ui {
                unsafe {
                    Image::capture(&mut self.ui, context, pass.ui_input, &mut self.generation)
                }?;
            }
            self.capture = Capture::Pending(scope);
        } else {
            let epoch = REQUEST.lock().ok()?.epoch;
            let scope = self
                .capture
                .take_pending(ScopeKey::new(context, pass, epoch))?;
            let source = if pass.color_output_readable.is_null() {
                pass.color_output
            } else {
                pass.color_output_readable
            };
            unsafe { Image::capture(&mut self.composed, context, source, &mut self.generation) }?;
            self.capture = Capture::Complete(scope);
        }
        Some(())
    }

    fn frame(&self) -> Option<Frame> {
        let Capture::Complete(scope) = self.capture else {
            return None;
        };
        let key = scope.key;
        if key.epoch != REQUEST.lock().ok()?.epoch {
            return None;
        }
        Some(Frame {
            struct_size: size_of::<Frame>() as u32,
            complete: 1,
            session_id: key.session_id,
            family_key: key.family_key,
            view_key: key.view_key,
            native_frame: key.native_frame,
            scope_id: key.scope_id,
            resource_generation: self.generation,
            output_rect: key.output_rect,
            scene: self.scene.as_ref()?.texture.as_raw(),
            ui_raster: if scope.has_ui {
                self.ui.as_ref()?.texture.as_raw()
            } else {
                ptr::null_mut()
            },
            composed: self.composed.as_ref()?.texture.as_raw(),
            source_frame_id: key.source_frame_id,
            submission_id: key.submission_id,
            flags: scope.flags,
            viewport_key: key.viewport_key,
        })
    }
}

static COMPOSITION: GraphicsCache<Composition> = GraphicsCache::new(Composition::new());

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_native_composition_capture(
    context: *mut c_void,
    pass: *const Pass,
    begin: u32,
) -> i32 {
    if context.is_null() || !unsafe { has_capacity(pass) } {
        return 0;
    }
    let pass = unsafe { &*pass };
    if pass.role != RSF_GAME_RENDER_UI_COMPOSITE
        || pass.flags & RSF_GAME_RENDER_PRIMARY == 0
        || pass.session_id == 0
        || pass.view_key == 0
        || pass.scope_id == 0
    {
        return 0;
    }
    COMPOSITION
        .with(|state| unsafe { state.capture(context, pass, begin != 0) })
        .flatten()
        .is_some()
        .into()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_composition_request(scopes: u32) {
    if let Ok(mut request) = REQUEST.lock() {
        request.remaining = scopes;
        if scopes == 0 {
            request.epoch = request.epoch.wrapping_add(1);
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_composition_read(frame: *mut Frame) -> i32 {
    if !unsafe { has_capacity(frame) } {
        return 0;
    }
    let Some(captured) = COMPOSITION.with(|state| state.frame()).flatten() else {
        return 0;
    };
    unsafe { frame.write(captured) };
    1
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_native_composition_release() {
    rsf_native_composition_request(0);
    COMPOSITION.with(|state| *state = Composition::new());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_scope_keys_on_a_different_stream_cannot_complete_capture() {
        let mut pass: Pass = unsafe { std::mem::zeroed() };
        pass.session_id = 1;
        pass.view_key = 2;
        pass.scope_id = 3;
        let begin = ScopeKey::new(ptr::dangling_mut::<u8>().cast(), &pass, 0);
        let wrong_stream = ScopeKey::new(ptr::dangling_mut::<u64>().cast(), &pass, 0);
        let mut capture = Capture::Pending(Scope {
            key: begin,
            flags: RSF_GAME_RENDER_PRIMARY,
            has_ui: false,
        });
        assert!(capture.take_pending(wrong_stream).is_none());
        assert!(capture.take_pending(begin).is_some());
        assert!(capture.take_pending(begin).is_none());
    }
}
