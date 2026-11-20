// SPDX-License-Identifier: MIT
use crate::fg_inputs_abi::{
    rsf_anomaly_input_constants as Constants, rsf_anomaly_input_desc as Description,
    rsf_anomaly_input_frame as Frame, rsf_anomaly_input_log as Log,
    rsf_anomaly_input_platform as Platform, rsf_anomaly_input_targets as Targets,
};
use crate::lease::Lease;
use std::ffi::c_void;
use std::ptr;

const R24G8_TYPELESS: u32 = 44;
const D24_UNORM_S8_UINT: u32 = 45;
const RGBA16_FLOAT: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Result {
    Ok,
    Shader,
    Format,
    Size,
    Resources,
}

struct SurfaceSet {
    resources: Lease,
    extent: [u32; 2],
    depth: *mut c_void,
    motion: *mut c_void,
}

pub(crate) struct Owner {
    platform: &'static Platform,
    surfaces: Option<SurfaceSet>,
    pipeline: Option<Lease>,
    device: Option<Lease>,
    shader_failed: bool,
    ready: bool,
}

impl Owner {
    pub(crate) fn new(platform: &'static Platform) -> Self {
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
        Some([set.depth, set.motion])
    }

    fn describe(&self, texture: *mut c_void) -> Description {
        let mut desc = Description::default();
        unsafe { (self.platform.describe.unwrap())(texture, &mut desc) };
        desc
    }

    fn ensure_targets(&mut self, device: *mut c_void, extent: [u32; 2]) -> Option<&SurfaceSet> {
        if self.surfaces.as_ref().is_none_or(|s| s.extent != extent) {
            let mut targets = Targets::default();
            unsafe { (self.platform.targets.unwrap())(device, extent[0], extent[1], &mut targets) };
            let resources = Lease::new(targets.resources, self.platform.release_targets.unwrap())?;
            if targets.depth.is_null() || targets.motion.is_null() {
                return None;
            }
            self.surfaces = Some(SurfaceSet {
                resources,
                extent,
                depth: targets.depth,
                motion: targets.motion,
            });
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

        let release_interface = platform.release_interface.unwrap();
        let Some(d3d) = Lease::new(
            unsafe { (platform.device.unwrap())(context) },
            release_interface,
        ) else {
            return Result::Resources;
        };
        let device = d3d.as_ptr();
        let same_device = platform.same_device.unwrap();
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
        if self.pipeline.is_none() {
            self.pipeline = Lease::new(
                unsafe { (platform.build.unwrap())(device, log, user) },
                platform.release_pipeline.unwrap(),
            );
        }
        let Some(pipeline) = self.pipeline.as_ref().map(Lease::as_ptr) else {
            self.shader_failed = true;
            return Result::Shader;
        };
        let Some(resources) = self
            .ensure_targets(device, extent)
            .map(|set| set.resources.as_ptr())
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
                    unsafe { (platform.motion_view.unwrap())(device, frame.motion) },
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
            (platform.dispatch.unwrap())(
                context,
                pipeline,
                resources,
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
mod tests;
