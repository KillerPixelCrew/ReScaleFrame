// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_output_abi::rsf_d3d11_state as State;
use crate::dxgi_format::{D32_FLOAT_S8X24_UINT, R32G8X24_TYPELESS};
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr::null_mut;
use std::sync::{Mutex, MutexGuard, TryLockError};

type Handle = *mut c_void;
type Resources = [Handle; 3];

// D3D11 values, asserted beside the native leaves.
const VIEW_TEXTURE2D: u32 = 3;
const READ_ONLY_DEPTH: u32 = 1;
const TRIANGLE_LIST: u32 = 4;
const TRIANGLE_STRIP: u32 = 5;
const WRITE_ZERO: u32 = 0;
const GREATER: u32 = 5;
const GREATER_EQUAL: u32 = 7;
const ALWAYS: u32 = 8;

// Reasons reported through rsf_depth_replay_last_reject.
const NO_INPUT: u32 = 1;
const SIZE: u32 = 2;
const SHAPE: u32 = 3;
const VIEW: u32 = 4;
const NOT_TEXTURE: u32 = 5;
const TEXTURE: u32 = 6;
const IDENTITY: u32 = 7;
const PIPELINE: u32 = 8;
const VIEWPORT: u32 = 9;

#[repr(C)]
struct Candidate {
    context: Handle,
    target: Handle,
    depth_view: Handle,
    width: u32,
    height: u32,
    samples: u32,
    kind: u32,
    count: u32,
    instances: u32,
    topology: u32,
    vertex_shader: u32,
    geometry: *const c_void,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Detail {
    draw_width: u32,
    draw_height: u32,
    draw_samples: u32,
    dsv_format: u32,
    dsv_dimension: u32,
    dsv_flags: u32,
    source_width: u32,
    source_height: u32,
    source_format: u32,
    source_samples: u32,
    layer: Handle,
    source: Handle,
    context: Handle,
    draws: u32,
    refused: u32,
}

#[repr(C)]
#[derive(Default)]
struct ViewInfo {
    format: u32,
    dimension: u32,
    flags: u32,
    mip_slice: u32,
}

#[repr(C)]
#[derive(Default)]
struct TextureInfo {
    width: u32,
    height: u32,
    format: u32,
    samples: u32,
    quality: u32,
    mips: u32,
    array: u32,
}

#[repr(C)]
#[derive(Default)]
struct PipelineInfo {
    stages: u32,
    stream_out: u32,
    uavs: u32,
    depth_state: u32,
    depth_enable: u32,
    depth_write_mask: u32,
    depth_func: u32,
    stencil_enable: u32,
    front_func: u32,
    back_func: u32,
    viewport_count: u32,
    viewport: [f32; 6],
}

#[repr(C)]
struct Platform {
    make: unsafe extern "C" fn(Handle, u32, u32, &mut Resources) -> i32,
    view: unsafe extern "C" fn(Handle, &mut ViewInfo),
    texture: unsafe extern "C" fn(Handle, &mut TextureInfo) -> Handle,
    pipeline: unsafe extern "C" fn(Handle, &mut PipelineInfo),
    save: unsafe extern "C" fn(Handle, *mut State) -> u32,
    restore: unsafe extern "C" fn(Handle, *mut State),
    unbind: unsafe extern "C" fn(Handle),
    copy: unsafe extern "C" fn(Handle, Handle, Handle),
    draw: unsafe extern "C" fn(Handle, &Resources, *const c_void),
}

const _: () = {
    assert!(size_of::<Candidate>() == 64 && size_of::<Detail>() == 72);
    assert!(size_of::<ViewInfo>() == 16 && size_of::<TextureInfo>() == 28);
    assert!(size_of::<PipelineInfo>() == 68);
    assert!(size_of::<Platform>() == 9 * size_of::<Handle>());
};

impl PipelineInfo {
    // Programmable geometry stays live, so stages with side effects or another geometry contract
    // are refused rather than disabled. Stencil writes of the translucency pass are omitted;
    // stencil tests would change coverage.
    fn supported(&self) -> bool {
        self.stages == 0
            && self.stream_out == 0
            && self.uavs == 0
            && self.depth_state != 0
            && self.depth_enable != 0
            && self.depth_write_mask == WRITE_ZERO
            && matches!(self.depth_func, GREATER | GREATER_EQUAL)
            && (self.stencil_enable == 0 || (self.front_func == ALWAYS && self.back_func == ALWAYS))
    }

    fn full_viewport(&self, width: u32, height: u32) -> bool {
        self.viewport_count == 1
            && self.viewport == [0.0, 0.0, width as f32, height as f32, 0.0, 1.0]
    }
}

fn drawable(candidate: &Candidate) -> bool {
    candidate.kind <= 3
        && candidate.vertex_shader != 0
        && candidate.count != 0
        && candidate.instances != 0
        && matches!(candidate.topology, TRIANGLE_LIST | TRIANGLE_STRIP)
}

#[derive(Default)]
struct Frame {
    // Retained texture behind the first accepted depth view of this frame.
    source: Option<ComRef>,
    layer: Option<ComRef>,
    // Compared only, never dereferenced.
    context: usize,
    draws: u32,
    refused: bool,
    // Why the last draw was not replayed. Five tests can refuse one, so it is reported.
    last_reject: u32,
    // What the last candidate's view and its resource actually were.
    detail: Detail,
}

impl Frame {
    fn refuse(&mut self, why: u32) -> u32 {
        self.refused = true;
        self.last_reject = why;
        0
    }

    fn selects(&self, context: Handle, depth: Handle, layer: Handle) -> bool {
        !self.refused
            && self.draws != 0
            && !layer.is_null()
            && self.layer.as_ref().map(ComRef::as_ptr) == Some(layer)
            && self.source.as_ref().map(ComRef::as_ptr) == Some(depth)
            && self.context == context as usize
    }
}

struct DepthReplay {
    resources: [ComRef; 3],
    width: u32,
    height: u32,
    frame: Mutex<Frame>,
    platform: &'static Platform,
}

impl DepthReplay {
    fn frame(&self) -> MutexGuard<'_, Frame> {
        self.frame.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn draw(&self, frame: &mut Frame, candidate: &Candidate) -> u32 {
        let platform = self.platform;
        let context = candidate.context;
        let mut view = ViewInfo::default();
        unsafe { (platform.view)(candidate.depth_view, &mut view) };
        let detail = &mut frame.detail;
        detail.dsv_format = view.format;
        detail.dsv_dimension = view.dimension;
        detail.dsv_flags = view.flags;
        detail.draw_width = candidate.width;
        detail.draw_height = candidate.height;
        detail.draw_samples = candidate.samples;
        // Sticky from here: these describe the frame rather than one draw.
        if view.format != D32_FLOAT_S8X24_UINT
            || view.dimension != VIEW_TEXTURE2D
            || view.mip_slice != 0
            || view.flags & READ_ONLY_DEPTH == 0
        {
            return frame.refuse(VIEW);
        }
        let mut info = TextureInfo::default();
        let texture =
            unsafe { ComRef::from_owned((platform.texture)(candidate.depth_view, &mut info)) };
        let Some(texture) = texture else {
            return frame.refuse(NOT_TEXTURE);
        };
        let detail = &mut frame.detail;
        detail.source_width = info.width;
        detail.source_height = info.height;
        detail.source_format = info.format;
        detail.source_samples = info.samples;
        if info.width != self.width
            || info.height != self.height
            || info.format != R32G8X24_TYPELESS
            || info.samples != 1
            || info.quality != 0
            || info.mips != 1
            || info.array != 1
        {
            return frame.refuse(TEXTURE);
        }
        // The first accepted view fixes this frame's source, layer and context.
        let same = frame.source.as_ref().map(|source| {
            source.as_ptr() == texture.as_ptr()
                && frame.layer.as_ref().map(ComRef::as_ptr) == Some(candidate.target)
                && frame.context == context as usize
        });
        match same {
            None => {
                frame.source = Some(texture);
                frame.layer = Some(unsafe { ComRef::retain(candidate.target) });
                frame.context = context as usize;
            }
            Some(false) => return frame.refuse(IDENTITY),
            Some(true) => {}
        }
        let mut pipeline = PipelineInfo::default();
        unsafe { (platform.pipeline)(context, &mut pipeline) };
        if !pipeline.supported() {
            return frame.refuse(PIPELINE);
        }
        if !pipeline.full_viewport(self.width, self.height) {
            return frame.refuse(VIEWPORT);
        }
        let Some(source) = frame.source.as_ref().map(ComRef::as_ptr) else {
            return 0;
        };
        let resources: Resources = std::array::from_fn(|i| self.resources[i].as_ptr());
        // A narrow scope: output targets, depth state and pixel shader only.
        let mut saved = MaybeUninit::<State>::uninit();
        if unsafe { (platform.save)(context, saved.as_mut_ptr()) } == 0 {
            frame.last_reject = PIPELINE;
            return 0;
        }
        unsafe {
            (platform.unbind)(context);
            if frame.draws == 0 {
                (platform.copy)(context, resources[0], source);
            }
            (platform.draw)(context, &resources, candidate.geometry);
            (platform.restore)(context, saved.as_mut_ptr());
        }
        frame.draws = frame.draws.saturating_add(1);
        1
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_depth_replay_create(
    device: Handle,
    width: u32,
    height: u32,
    platform: &'static Platform,
) -> *mut DepthReplay {
    if device.is_null() || width == 0 || height == 0 {
        return null_mut();
    }
    let mut raw = [null_mut(); 3];
    let made = unsafe { (platform.make)(device, width, height, &mut raw) } != 0;
    let resources = unsafe { ComRef::adopt_all(raw) };
    if !made {
        return null_mut();
    }
    let Some(resources) = resources else {
        return null_mut();
    };
    crate::allocate_handle(DepthReplay {
        resources,
        width,
        height,
        frame: Mutex::default(),
        platform,
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_depth_replay_last_reject(replay: *const DepthReplay) -> u32 {
    unsafe { replay.as_ref() }.map_or(0, |replay| replay.frame().last_reject)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_depth_replay_get_detail(
    replay: *const DepthReplay,
    out: *mut Detail,
) {
    if out.is_null() {
        return;
    }
    let detail = unsafe { replay.as_ref() }.map_or_else(Detail::default, |replay| {
        let frame = replay.frame();
        Detail {
            layer: frame.layer.as_ref().map_or(null_mut(), ComRef::as_ptr),
            source: frame.source.as_ref().map_or(null_mut(), ComRef::as_ptr),
            context: frame.context as Handle,
            draws: frame.draws,
            refused: frame.refused.into(),
            ..frame.detail
        }
    });
    unsafe { out.write(detail) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_depth_replay_draw(
    replay: *const DepthReplay,
    candidate: *const Candidate,
) -> u32 {
    let Some(replay) = (unsafe { replay.as_ref() }) else {
        return 0;
    };
    // The tap suppresses reentry; refuse rather than wait if one gets through.
    let mut frame = match replay.frame.try_lock() {
        Ok(frame) => frame,
        Err(TryLockError::Poisoned(error)) => error.into_inner(),
        Err(TryLockError::WouldBlock) => return 0,
    };
    let candidate = unsafe { candidate.as_ref() }.filter(|candidate| {
        !candidate.context.is_null()
            && !candidate.depth_view.is_null()
            && !candidate.target.is_null()
    });
    let Some(candidate) = candidate else {
        frame.last_reject = NO_INPUT;
        return 0;
    };
    if candidate.width != replay.width
        || candidate.height != replay.height
        || candidate.samples != 1
    {
        frame.last_reject = SIZE;
        return 0;
    }
    if frame.refused {
        return 0;
    }
    // Not sticky: the next draw is a different one.
    if !drawable(candidate) {
        frame.last_reject = SHAPE;
        return 0;
    }
    replay.draw(&mut frame, candidate)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_depth_replay_selected(
    replay: *const DepthReplay,
    context: Handle,
    depth: Handle,
    layer: Handle,
) -> Handle {
    match unsafe { replay.as_ref() } {
        Some(replay) if replay.frame().selects(context, depth, layer) => {
            replay.resources[0].as_ptr()
        }
        _ => depth,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_depth_replay_end_frame(replay: *const DepthReplay) {
    if let Some(replay) = unsafe { replay.as_ref() } {
        let mut frame = replay.frame();
        frame.source = None;
        frame.layer = None;
        frame.context = 0;
        frame.draws = 0;
        frame.refused = false;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_depth_replay_destroy(replay: *mut DepthReplay) {
    if !replay.is_null() {
        drop(unsafe { Box::from_raw(replay) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn translucency() -> PipelineInfo {
        PipelineInfo {
            depth_state: 1,
            depth_enable: 1,
            depth_func: GREATER_EQUAL,
            viewport_count: 1,
            viewport: [0.0, 0.0, 64.0, 32.0, 0.0, 1.0],
            ..PipelineInfo::default()
        }
    }

    #[test]
    fn pipeline_acceptance_keeps_the_translucency_contract() {
        let pipeline = translucency();
        assert!(pipeline.supported() && pipeline.full_viewport(64, 32));
        assert!(!pipeline.full_viewport(32, 32));
        let stencil_test = PipelineInfo {
            stencil_enable: 1,
            front_func: ALWAYS,
            back_func: 3,
            ..translucency()
        };
        assert!(!stencil_test.supported());
        let writes = PipelineInfo {
            depth_write_mask: 1,
            ..translucency()
        };
        assert!(!writes.supported());
        let geometry = PipelineInfo {
            stages: 1,
            ..translucency()
        };
        assert!(!geometry.supported());
    }
}
