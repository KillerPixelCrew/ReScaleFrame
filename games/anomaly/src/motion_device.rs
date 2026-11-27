// SPDX-License-Identifier: MIT
use crate::lease::Lease;
use crate::motion::{CORE_BYTES, FOLIAGE_BYTES, PIXEL_BYTES, TESS_BYTES};
use crate::sys::{
    RSF_MOTION_CORE, RSF_MOTION_CREATED, RSF_MOTION_FAILED, RSF_MOTION_FOLIAGE,
    RSF_MOTION_MULTISAMPLED, RSF_MOTION_PIXEL, RSF_MOTION_READY, RSF_MOTION_TESS,
    rsf_anomaly_motion_device_view as View, rsf_anomaly_motion_log as Log,
};
#[cfg(windows)]
use rsf_game_sys::rsf_shader_patch_api;
use rsf_game_sys::{RSF_PATCH_OK, rsf_patch_bytes};
use std::ffi::{c_char, c_void};

#[cfg(windows)]
mod d3d11;
mod replay;
#[cfg(windows)]
pub(crate) use crate::com::{release, retain};
#[cfg(windows)]
pub(crate) use d3d11::{
    clear_target, rsf_anomaly_motion_bind_targets, rsf_anomaly_motion_clear_targets,
};
#[cfg(windows)]
pub(crate) use replay::{
    REPLAY_NO_DEPTH_WRITE, REPLAY_READY, ReplayScope, rsf_anomaly_motion_replay_begin,
    rsf_anomaly_motion_replay_end,
};

struct Buffer {
    resource: Lease,
    last: Box<[u8]>,
    stage: c_char,
    uploaded: bool,
}

struct Target {
    view: Lease,
    texture: Lease,
    extent: [u32; 2],
}

type Raw = *mut c_void;

// The D3D11 resource leaves: d3d11.rs, the counting fakes under test.
#[cfg(all(windows, not(test)))]
use d3d11::device as leaves;
#[cfg(test)]
use tests as leaves;

#[derive(Clone, Copy)]
pub(crate) struct ConstantOps {
    buffer: unsafe extern "C" fn(Raw, u32, *mut Raw) -> i32,
    upload: unsafe extern "C" fn(Raw, Raw, rsf_patch_bytes) -> i32,
    bind: unsafe extern "C" fn(Raw, u32, u32, Raw) -> i32,
}

#[cfg(windows)]
impl ConstantOps {
    pub(crate) fn from_api(api: &rsf_shader_patch_api) -> Option<Self> {
        Some(Self {
            buffer: api.constant_buffer?,
            upload: api.upload_constant?,
            bind: api.bind_constant?,
        })
    }
}

struct Device {
    target: Option<Target>,
    buffers: [Buffer; 4],
    zero_shader: Lease,
    no_depth: Lease,
    equal_depth: Lease,
    device: Lease,
    runtime: Option<ConstantOps>,
}

impl Device {
    fn new(pointer: Raw, runtime: Option<ConstantOps>, log: Log, user: Raw) -> Option<Self> {
        let release = leaves::release;
        let device = Lease::new(pointer, release)?;
        unsafe { leaves::retain(pointer) };
        let buffer = |size: usize, stage: u8| {
            let resource = if let Some(runtime) = runtime {
                let mut handle = std::ptr::null_mut();
                let result = unsafe { (runtime.buffer)(pointer, size as u32, &mut handle) };
                let lease = Lease::new(handle, release);
                if result != RSF_PATCH_OK {
                    return None;
                }
                lease?
            } else {
                let handle = unsafe { leaves::buffer(pointer, size as u32) };
                Lease::new(handle, release)?
            };
            Some(Buffer {
                resource,
                last: vec![0; size].into_boxed_slice(),
                stage: stage as c_char,
                uploaded: false,
            })
        };
        let buffers = [
            buffer(CORE_BYTES, b'v')?,
            buffer(FOLIAGE_BYTES, b'v')?,
            buffer(PIXEL_BYTES, b'p')?,
            buffer(TESS_BYTES, b'd')?,
        ];
        let shader = unsafe { leaves::zero_shader(pointer, log, user) };
        let zero_shader = Lease::new(shader, release)?;
        let depth = |equal| {
            let handle = unsafe { leaves::depth_state(pointer, equal) };
            Lease::new(handle, release)
        };
        Some(Self {
            target: None,
            buffers,
            zero_shader,
            no_depth: depth(0)?,
            equal_depth: depth(1)?,
            device,
            runtime,
        })
    }

    fn ensure_target(&mut self, extent: [u32; 2]) -> Option<i32> {
        if self.target.as_ref().is_some_and(|t| t.extent == extent) {
            return Some(RSF_MOTION_READY);
        }
        self.target = None;
        let device = self.device.as_ptr();
        let release = leaves::release;
        let texture = unsafe { leaves::texture(device, extent[0], extent[1]) };
        let texture = Lease::new(texture, release)?;
        let view = unsafe { leaves::view(device, texture.as_ptr()) };
        self.target = Some(Target {
            view: Lease::new(view, release)?,
            texture,
            extent,
        });
        Some(RSF_MOTION_CREATED)
    }
}

pub(crate) struct Owner {
    device: Option<Device>,
    uploads: u64,
    upload_skips: u64,
}

impl Owner {
    pub(crate) fn new() -> Self {
        Self {
            device: None,
            uploads: 0,
            upload_skips: 0,
        }
    }

    pub(crate) fn create(
        &mut self,
        device: Raw,
        runtime: Option<ConstantOps>,
        log: Log,
        user: Raw,
    ) -> bool {
        self.device = None;
        self.device = Device::new(device, runtime, log, user);
        self.device.is_some()
    }

    /// Sizes the motion target: RSF_MOTION_READY, _CREATED, _MULTISAMPLED or _FAILED.
    pub(crate) fn target(&mut self, width: u32, height: u32, samples: u32, layers: u32) -> i32 {
        let Some(device) = &mut self.device else {
            return RSF_MOTION_FAILED;
        };
        if samples != 1 || layers != 1 {
            return RSF_MOTION_MULTISAMPLED;
        }
        device
            .ensure_target([width, height])
            .unwrap_or(RSF_MOTION_FAILED)
    }

    pub(crate) fn view(&self) -> View {
        let mut out = View {
            uploads: self.uploads,
            upload_skips: self.upload_skips,
            ..View::default()
        };
        let Some(device) = &self.device else {
            return out;
        };
        out.device = device.device.as_ptr();
        out.zero_shader = device.zero_shader.as_ptr();
        out.no_depth = device.no_depth.as_ptr();
        out.equal_depth = device.equal_depth.as_ptr();
        if let Some(target) = &device.target {
            out.rtv = target.view.as_ptr();
            out.texture = target.texture.as_ptr();
            [out.width, out.height] = target.extent;
        }
        out
    }

    /// Uploads one b13 block (skipped when unchanged) and binds it.
    pub(crate) fn upload(&mut self, context: Raw, kind: i32, bytes: &[u8]) -> bool {
        if context.is_null() {
            return false;
        }
        let Some(device) = &mut self.device else {
            return false;
        };
        let index = match kind {
            RSF_MOTION_CORE => 0,
            RSF_MOTION_FOLIAGE => 1,
            RSF_MOTION_PIXEL => 2,
            RSF_MOTION_TESS => 3,
            _ => return false,
        };
        let buffer = &mut device.buffers[index];
        if bytes.len() != buffer.last.len() {
            return false;
        }
        let resource = buffer.resource.as_ptr();
        if buffer.uploaded && *buffer.last == *bytes {
            self.upload_skips += 1;
        } else {
            buffer.uploaded = if let Some(runtime) = device.runtime {
                let input = rsf_patch_bytes {
                    data: bytes.as_ptr().cast(),
                    size: bytes.len(),
                };
                unsafe { (runtime.upload)(context, resource, input) == RSF_PATCH_OK }
            } else {
                unsafe {
                    leaves::upload(context, resource, bytes.as_ptr().cast(), bytes.len()) != 0
                }
            };
            if !buffer.uploaded {
                return false;
            }
            buffer.last.copy_from_slice(bytes);
            self.uploads += 1;
        }
        if let Some(runtime) = device.runtime {
            unsafe { (runtime.bind)(context, buffer.stage as u32, 13, resource) == RSF_PATCH_OK }
        } else {
            unsafe { leaves::bind(context, resource, buffer.stage) != 0 }
        }
    }
}

#[cfg(test)]
mod tests;
