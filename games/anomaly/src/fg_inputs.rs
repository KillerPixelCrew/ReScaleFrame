// SPDX-License-Identifier: MIT
use crate::fg_inputs_abi::{
    rsf_anomaly_input_constants as Constants, rsf_anomaly_input_desc as Description,
    rsf_anomaly_input_frame as Frame, rsf_anomaly_input_log as Log,
    rsf_anomaly_input_pipeline as PipelineBindings, rsf_anomaly_input_platform as Platform,
    rsf_anomaly_input_targets as Targets,
};
use crate::lease::Lease;
use std::ffi::c_void;
use std::ptr;

#[derive(Clone, Copy)]
pub(crate) struct Bindings {
    device: unsafe extern "C" fn(context: *mut c_void) -> *mut c_void,
    pub(crate) release_interface: unsafe extern "C" fn(arg1: *mut c_void),
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
    dispatch: unsafe extern "C" fn(
        context: *mut c_void,
        pipeline: *const PipelineBindings,
        targets: *const Targets,
        depth: *mut c_void,
        motion_view: *mut c_void,
        arg1: *const Constants,
        groups_x: u32,
        groups_y: u32,
    ) -> u32,
    first_person_limit: f32,
    validity_threshold: f32,
}

impl Bindings {
    pub(crate) fn bind(platform: &Platform) -> Option<Self> {
        if platform.struct_size < size_of::<Platform>() as u32 {
            return None;
        }
        Some(Self {
            device: platform.device?,
            release_interface: platform.release_interface?,
            describe: platform.describe?,
            same_device: platform.same_device?,
            shader: platform.shader?,
            constant_buffer: platform.constant_buffer?,
            texture: platform.texture?,
            depth_view: platform.depth_view?,
            unordered_view: platform.unordered_view?,
            motion_view: platform.motion_view?,
            dispatch: platform.dispatch?,
            first_person_limit: platform.first_person_limit,
            validity_threshold: platform.validity_threshold,
        })
    }
}

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
    surfaces: Option<SurfaceSet>,
    pipeline: Option<Pipeline>,
    device: Option<Lease>,
    shader_failed: bool,
    ready: bool,
}

impl Owner {
    pub(crate) fn new(platform: Bindings) -> Self {
        Self {
            platform,
            surfaces: None,
            pipeline: None,
            device: None,
            shader_failed: false,
            ready: false,
        }
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
            first_person_limit: platform.first_person_limit,
            validity_threshold: platform.validity_threshold,
            ..Default::default()
        };
        let accepted = unsafe {
            (platform.dispatch)(
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
