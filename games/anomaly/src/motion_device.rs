// SPDX-License-Identifier: MIT
use crate::lease::Lease;
use crate::motion::{CORE_BYTES, FOLIAGE_BYTES, PIXEL_BYTES, TESS_BYTES};
use crate::motion_device_abi::{
    RSF_MOTION_CORE, RSF_MOTION_CREATED, RSF_MOTION_FAILED, RSF_MOTION_FOLIAGE,
    RSF_MOTION_MULTISAMPLED, RSF_MOTION_PIXEL, RSF_MOTION_READY, RSF_MOTION_TESS,
    rsf_anomaly_motion_device_platform as Platform, rsf_anomaly_motion_device_view as View,
    rsf_anomaly_motion_log as Log,
};
use std::ffi::{c_char, c_void};
use std::mem::MaybeUninit;
use std::slice;

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

struct Device {
    target: Option<Target>,
    buffers: [Buffer; 4],
    zero_shader: Lease,
    no_depth: Lease,
    equal_depth: Lease,
    device: Lease,
}

impl Device {
    fn new(platform: &Platform, pointer: *mut c_void, log: Log, user: *mut c_void) -> Option<Self> {
        let release = platform.release.unwrap();
        let device = Lease::new(pointer, release)?;
        unsafe { (platform.retain.unwrap())(pointer) };
        let buffer = |size: usize, stage: u8| {
            let handle = unsafe { (platform.buffer.unwrap())(pointer, size as u32) };
            Some(Buffer {
                resource: Lease::new(handle, release)?,
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
        let shader = unsafe { (platform.zero_shader.unwrap())(pointer, log, user) };
        let zero_shader = Lease::new(shader, release)?;
        let depth = |equal| {
            let handle = unsafe { (platform.depth_state.unwrap())(pointer, equal) };
            Lease::new(handle, release)
        };
        Some(Self {
            target: None,
            buffers,
            zero_shader,
            no_depth: depth(0)?,
            equal_depth: depth(1)?,
            device,
        })
    }

    fn ensure_target(&mut self, platform: &Platform, extent: [u32; 2]) -> Option<i32> {
        if self.target.as_ref().is_some_and(|t| t.extent == extent) {
            return Some(RSF_MOTION_READY);
        }
        self.target = None;
        let device = self.device.as_ptr();
        let release = platform.release.unwrap();
        let texture = unsafe { (platform.texture.unwrap())(device, extent[0], extent[1]) };
        let texture = Lease::new(texture, release)?;
        let view = unsafe { (platform.view.unwrap())(device, texture.as_ptr()) };
        self.target = Some(Target {
            view: Lease::new(view, release)?,
            texture,
            extent,
        });
        Some(RSF_MOTION_CREATED)
    }
}

struct Owner {
    platform: &'static Platform,
    device: Option<Device>,
    uploads: u64,
    upload_skips: u64,
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_device_new(platform: &'static Platform) -> *mut Owner {
    Box::into_raw(Box::new(Owner {
        platform,
        device: None,
        uploads: 0,
        upload_skips: 0,
    }))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_device_drop(owner: *mut Owner) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_device_reset(owner: Option<&mut Owner>) {
    if let Some(owner) = owner {
        owner.device = None;
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_device_create(
    owner: &mut Owner,
    device: *mut c_void,
    log: Log,
    user: *mut c_void,
) -> u32 {
    owner.device = None;
    owner.device = Device::new(owner.platform, device, log, user);
    u32::from(owner.device.is_some())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_device_target(
    owner: Option<&mut Owner>,
    width: u32,
    height: u32,
    samples: u32,
    layers: u32,
) -> i32 {
    let Some(owner) = owner else {
        return RSF_MOTION_FAILED;
    };
    let Some(device) = &mut owner.device else {
        return RSF_MOTION_FAILED;
    };
    if samples != 1 || layers != 1 {
        return RSF_MOTION_MULTISAMPLED;
    }
    device
        .ensure_target(owner.platform, [width, height])
        .unwrap_or(RSF_MOTION_FAILED)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_device_get(owner: Option<&Owner>, out: &mut MaybeUninit<View>) {
    let out = out.write(View::default());
    let Some(owner) = owner else { return };
    out.uploads = owner.uploads;
    out.upload_skips = owner.upload_skips;
    let Some(device) = &owner.device else { return };
    out.device = device.device.as_ptr();
    out.zero_shader = device.zero_shader.as_ptr();
    out.no_depth = device.no_depth.as_ptr();
    out.equal_depth = device.equal_depth.as_ptr();
    if let Some(target) = &device.target {
        out.rtv = target.view.as_ptr();
        out.texture = target.texture.as_ptr();
        [out.width, out.height] = target.extent;
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_device_upload(
    owner: Option<&mut Owner>,
    context: *mut c_void,
    kind: i32,
    data: *const u8,
    size: usize,
) -> u32 {
    if context.is_null() || data.is_null() {
        return 0;
    }
    let Some(owner) = owner else { return 0 };
    let Some(device) = &mut owner.device else {
        return 0;
    };
    let index = match kind {
        RSF_MOTION_CORE => 0,
        RSF_MOTION_FOLIAGE => 1,
        RSF_MOTION_PIXEL => 2,
        RSF_MOTION_TESS => 3,
        _ => return 0,
    };
    let buffer = &mut device.buffers[index];
    if size != buffer.last.len() {
        return 0;
    }
    let bytes = unsafe { slice::from_raw_parts(data, size) };
    let resource = buffer.resource.as_ptr();
    if buffer.uploaded && *buffer.last == *bytes {
        owner.upload_skips += 1;
    } else {
        let upload = owner.platform.upload.unwrap();
        buffer.uploaded = unsafe { upload(context, resource, data.cast(), size) } != 0;
        if !buffer.uploaded {
            return 0;
        }
        buffer.last.copy_from_slice(bytes);
        owner.uploads += 1;
    }
    unsafe { (owner.platform.bind.unwrap())(context, resource, buffer.stage) }
}

#[cfg(test)]
mod tests;
