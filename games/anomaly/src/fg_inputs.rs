// SPDX-License-Identifier: MIT
#[cfg(windows)]
use crate::fg_stage_abi::{rsf_anomaly_fg_state_scope, rsf_d3d11_state};
use crate::lease::Lease;
use std::ffi::c_void;
use std::ptr;

#[cfg(all(windows, not(test)))]
pub(crate) mod d3d;

type Log = rsf_game_sys::rsf_game_log_fn;

#[derive(Copy, Clone)]
pub(crate) struct Frame {
    pub depth: *mut c_void,
    pub motion: *mut c_void,
    pub clip_to_previous_clip: [f32; 16],
    pub width: u32,
    pub height: u32,
}

#[derive(Default, Copy, Clone)]
struct Description {
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub layers: u32,
    pub mips: u32,
}

#[derive(Copy, Clone)]
struct Targets {
    pub depth_copy: *mut c_void,
    // The full target set; dispatch receives the depth and motion views separately.
    #[allow(dead_code)]
    pub depth: *mut c_void,
    #[allow(dead_code)]
    pub motion: *mut c_void,
    pub depth_view: *mut c_void,
    pub depth_uav: *mut c_void,
    pub motion_uav: *mut c_void,
}

#[derive(Copy, Clone)]
struct PipelineBindings {
    pub shader: *mut c_void,
    pub constants: *mut c_void,
}

#[repr(C)]
#[derive(Default, Copy, Clone)]
struct Constants {
    pub clip_to_previous: [f32; 16],
    pub size: [f32; 2],
    pub has_motion: f32,
    pub first_person_limit: f32,
    pub validity_threshold: f32,
    pub padding: [f32; 3],
}

const _: () = assert!(size_of::<Constants>() == 96);

#[derive(Clone, Copy)]
struct Bindings {
    device: unsafe extern "C" fn(context: *mut c_void) -> *mut c_void,
    release_interface: unsafe extern "C" fn(arg1: *mut c_void),
    describe: unsafe extern "C" fn(texture: *mut c_void, arg1: *mut Description),
    same_device: unsafe extern "C" fn(texture: *mut c_void, device: *mut c_void) -> u32,
    shader: unsafe extern "C" fn(device: *mut c_void, arg1: Log, arg2: *mut c_void) -> *mut c_void,
    constant_buffer: unsafe extern "C" fn(device: *mut c_void) -> *mut c_void,
    texture: unsafe extern "C" fn(
        device: *mut c_void,
        width: u32,
        height: u32,
        format: u32,
        bind: u32,
    ) -> *mut c_void,
    depth_view: unsafe extern "C" fn(device: *mut c_void, depth_copy: *mut c_void) -> *mut c_void,
    unordered_view: unsafe extern "C" fn(device: *mut c_void, texture: *mut c_void) -> *mut c_void,
    motion_view: unsafe extern "C" fn(device: *mut c_void, texture: *mut c_void) -> *mut c_void,
    #[allow(clippy::type_complexity)]
    dispatch: unsafe fn(
        state_scope: StateScope,
        context: *mut c_void,
        pipeline: *const PipelineBindings,
        targets: *const Targets,
        depth: *mut c_void,
        motion_view: *mut c_void,
        arg1: *const Constants,
        groups_x: u32,
        groups_y: u32,
    ) -> u32,
}

#[cfg(windows)]
#[derive(Clone, Copy)]
#[cfg_attr(test, allow(dead_code))]
pub(crate) struct StateScope {
    #[cfg_attr(test, allow(dead_code))]
    pub(super) save: unsafe extern "C" fn(*mut c_void, *mut rsf_d3d11_state) -> u32,
    #[cfg_attr(test, allow(dead_code))]
    pub(super) restore: unsafe extern "C" fn(*mut c_void, *mut rsf_d3d11_state),
}

#[cfg(windows)]
impl StateScope {
    pub(crate) fn bind(scope: rsf_anomaly_fg_state_scope) -> Option<Self> {
        Some(Self {
            save: scope.save?,
            restore: scope.restore?,
        })
    }
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
struct StateScope;

const R24G8_TYPELESS: u32 = 44;
const D24_UNORM_S8_UINT: u32 = 45;
const RGBA16_FLOAT: u32 = 10;
const R32_FLOAT: u32 = 41;
const RG16_FLOAT: u32 = 34;
const BIND_SHADER_RESOURCE: u32 = 0x8;
const BIND_UNORDERED_ACCESS: u32 = 0x80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Result {
    Ok,
    Shader,
    Format,
    Size,
    Resources,
}

/// One complete staging set. Fields drop in declaration order, which is the reverse of creation and
/// matches the destruction order of the former C++ `Targets` members.
struct SurfaceSet {
    motion_uav: Lease,
    depth_uav: Lease,
    depth_view: Lease,
    motion: Lease,
    depth: Lease,
    depth_copy: Lease,
    extent: [u32; 2],
}

impl SurfaceSet {
    /// Creates the textures, then their views. A failed create drops the leases already taken in
    /// reverse order and returns None, so a caller keeps its previous complete set.
    fn create(platform: &Bindings, device: *mut c_void, extent: [u32; 2]) -> Option<Self> {
        let release = platform.release_interface;
        let create_texture = platform.texture;
        let create_depth_view = platform.depth_view;
        let create_uav = platform.unordered_view;
        let make = |format: u32, bind: u32| {
            Lease::new(
                unsafe { create_texture(device, extent[0], extent[1], format, bind) },
                release,
            )
        };
        let output_bind = BIND_SHADER_RESOURCE | BIND_UNORDERED_ACCESS;
        let depth_copy = make(R24G8_TYPELESS, BIND_SHADER_RESOURCE)?;
        let depth = make(R32_FLOAT, output_bind)?;
        let motion = make(RG16_FLOAT, output_bind)?;
        let depth_view = Lease::new(
            unsafe { create_depth_view(device, depth_copy.as_ptr()) },
            release,
        )?;
        let depth_uav = Lease::new(unsafe { create_uav(device, depth.as_ptr()) }, release)?;
        let motion_uav = Lease::new(unsafe { create_uav(device, motion.as_ptr()) }, release)?;
        Some(Self {
            motion_uav,
            depth_uav,
            depth_view,
            motion,
            depth,
            depth_copy,
            extent,
        })
    }

    /// Borrows the handles for one dispatch; the set keeps every reference.
    fn bindings(&self) -> Targets {
        Targets {
            depth_copy: self.depth_copy.as_ptr(),
            depth: self.depth.as_ptr(),
            motion: self.motion.as_ptr(),
            depth_view: self.depth_view.as_ptr(),
            depth_uav: self.depth_uav.as_ptr(),
            motion_uav: self.motion_uav.as_ptr(),
        }
    }
}

struct Pipeline {
    // The constant buffer drops before the shader, as in the native pipeline.
    constants: Lease,
    shader: Lease,
}

impl Pipeline {
    fn create(
        platform: &Bindings,
        device: *mut c_void,
        log: Log,
        user: *mut c_void,
    ) -> std::result::Result<Self, Result> {
        let release = platform.release_interface;
        let shader = Lease::new(unsafe { (platform.shader)(device, log, user) }, release)
            .ok_or(Result::Shader)?;
        let constants = Lease::new(unsafe { (platform.constant_buffer)(device) }, release)
            .ok_or(Result::Resources)?;
        Ok(Self { constants, shader })
    }

    fn bindings(&self) -> PipelineBindings {
        PipelineBindings {
            shader: self.shader.as_ptr(),
            constants: self.constants.as_ptr(),
        }
    }
}

pub(crate) struct Owner {
    platform: Bindings,
    state_scope: StateScope,
    surfaces: Option<SurfaceSet>,
    pipeline: Option<Pipeline>,
    device: Option<Lease>,
    shader_failed: bool,
    ready: bool,
}

impl Owner {
    fn with_bindings(platform: Bindings, state_scope: StateScope) -> Self {
        Self {
            platform,
            state_scope,
            surfaces: None,
            pipeline: None,
            device: None,
            shader_failed: false,
            ready: false,
        }
    }

    #[cfg(windows)]
    pub(crate) fn new(state_scope: StateScope) -> Self {
        #[cfg(test)]
        let bindings = tests::BINDINGS;
        #[cfg(not(test))]
        let bindings = d3d::BINDINGS;
        Self::with_bindings(bindings, state_scope)
    }

    pub(crate) fn outputs(&self) -> Option<[*mut c_void; 2]> {
        let set = self.surfaces.as_ref().filter(|_| self.ready)?;
        Some([set.depth.as_ptr(), set.motion.as_ptr()])
    }

    fn describe(&self, texture: *mut c_void) -> Description {
        let mut desc = Description::default();
        unsafe { (self.platform.describe)(texture, &mut desc) };
        desc
    }

    fn ensure_targets(&mut self, device: *mut c_void, extent: [u32; 2]) -> Option<&SurfaceSet> {
        if self.surfaces.as_ref().is_none_or(|s| s.extent != extent) {
            // The new set is complete before the older one drops.
            self.surfaces = Some(SurfaceSet::create(&self.platform, device, extent)?);
        }
        self.surfaces.as_ref()
    }

    pub(crate) fn run(
        &mut self,
        context: *mut c_void,
        frame: &Frame,
        log: Log,
        user: *mut c_void,
    ) -> Result {
        let platform = self.platform;
        self.ready = false;
        if context.is_null() || frame.depth.is_null() || frame.width == 0 || frame.height == 0 {
            return Result::Resources;
        }
        let depth = self.describe(frame.depth);
        if !matches!(depth.format, R24G8_TYPELESS | D24_UNORM_S8_UINT)
            || depth.samples != 1
            || depth.layers != 1
            || depth.mips != 1
        {
            return Result::Format;
        }
        let extent = [frame.width, frame.height];
        if [depth.width, depth.height] != extent {
            return Result::Size;
        }

        let release_interface = platform.release_interface;
        let Some(d3d) = Lease::new(unsafe { (platform.device)(context) }, release_interface) else {
            return Result::Resources;
        };
        let device = d3d.as_ptr();
        let same_device = platform.same_device;
        if unsafe { same_device(frame.depth, device) } == 0 {
            return Result::Resources;
        }
        if self
            .device
            .as_ref()
            .is_none_or(|old| old.as_ptr() != device)
        {
            self.surfaces = None;
            self.pipeline = None;
            self.device = Some(d3d);
            self.shader_failed = false;
        }
        if self.shader_failed {
            return Result::Shader;
        }
        let pipeline = match &self.pipeline {
            Some(pipeline) => pipeline.bindings(),
            None => match Pipeline::create(&platform, device, log, user) {
                Ok(pipeline) => {
                    let bindings = pipeline.bindings();
                    self.pipeline = Some(pipeline);
                    bindings
                }
                Err(result) => {
                    self.shader_failed = result == Result::Shader;
                    return result;
                }
            },
        };
        let Some(targets) = self
            .ensure_targets(device, extent)
            .map(SurfaceSet::bindings)
        else {
            return Result::Resources;
        };

        let motion_view = if frame.motion.is_null() {
            None
        } else {
            let desc = self.describe(frame.motion);
            if desc.format == RGBA16_FLOAT
                && [desc.width, desc.height] == extent
                && desc.samples == 1
                && desc.layers == 1
                && unsafe { same_device(frame.motion, device) } != 0
            {
                Lease::new(
                    unsafe { (platform.motion_view)(device, frame.motion) },
                    release_interface,
                )
            } else {
                // The shader reconstructs camera motion when an optional motion view is unavailable.
                None
            }
        };
        let constants = Constants {
            clip_to_previous: frame.clip_to_previous_clip,
            size: [frame.width as f32, frame.height as f32],
            has_motion: if motion_view.is_some() { 1.0 } else { 0.0 },
            first_person_limit: f32::from_bits(crate::sr_stage::FIRST_PERSON_DEPTH_BITS),
            validity_threshold: crate::sr_temporal::MOTION_VALIDITY_THRESHOLD,
            ..Default::default()
        };
        let accepted = unsafe {
            (platform.dispatch)(
                self.state_scope,
                context,
                &pipeline,
                &targets,
                frame.depth,
                motion_view.as_ref().map_or(ptr::null_mut(), Lease::as_ptr),
                &constants,
                frame.width.div_ceil(8),
                frame.height.div_ceil(8),
            )
        };
        self.ready = accepted != 0;
        if self.ready {
            Result::Ok
        } else {
            Result::Resources
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
