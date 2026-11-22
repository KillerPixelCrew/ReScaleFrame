// SPDX-License-Identifier: MIT
use crate::lease::Lease;
use crate::motion::{CORE_BYTES, FOLIAGE_BYTES, PIXEL_BYTES, TESS_BYTES};
use crate::motion_device_abi::{
    RSF_MOTION_CORE, RSF_MOTION_CREATED, RSF_MOTION_FAILED, RSF_MOTION_FOLIAGE,
    RSF_MOTION_MULTISAMPLED, RSF_MOTION_PIXEL, RSF_MOTION_READY, RSF_MOTION_TESS,
    rsf_anomaly_motion_device_platform as NativePlatform, rsf_anomaly_motion_device_view as View,
    rsf_anomaly_motion_log as Log,
};
#[cfg(windows)]
use rsf_game_sys::shader_patch_abi::rsf_shader_patch_api;
use rsf_game_sys::shader_patch_abi::{RSF_PATCH_OK, rsf_patch_bytes};
use std::ffi::{c_char, c_void};
use std::mem::MaybeUninit;
use std::slice;

#[cfg(windows)]
mod d3d11;
mod replay;
#[cfg(windows)]
pub(crate) use replay::{
    Platform as ReplayPlatform, REPLAY_NO_DEPTH_WRITE, REPLAY_READY, ReplayScope,
    rsf_anomaly_motion_replay_begin, rsf_anomaly_motion_replay_end,
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

#[repr(C)]
pub(crate) struct Platform {
    pub(crate) retain: unsafe extern "C" fn(Raw),
    pub(crate) release: unsafe extern "C" fn(Raw),
    buffer: unsafe extern "C" fn(Raw, u32) -> Raw,
    zero_shader: unsafe extern "C" fn(Raw, Log, Raw) -> Raw,
    depth_state: unsafe extern "C" fn(Raw, u32) -> Raw,
    texture: unsafe extern "C" fn(Raw, u32, u32) -> Raw,
    view: unsafe extern "C" fn(Raw, Raw) -> Raw,
    upload: unsafe extern "C" fn(Raw, Raw, *const c_void, usize) -> u32,
    bind: unsafe extern "C" fn(Raw, Raw, c_char) -> u32,
}

const _: () = {
    assert!(size_of::<Platform>() == size_of::<NativePlatform>());
    assert!(align_of::<Platform>() == align_of::<NativePlatform>());
    assert!(std::mem::offset_of!(Platform, retain) == std::mem::offset_of!(NativePlatform, retain));
    assert!(
        std::mem::offset_of!(Platform, release) == std::mem::offset_of!(NativePlatform, release)
    );
    assert!(std::mem::offset_of!(Platform, buffer) == std::mem::offset_of!(NativePlatform, buffer));
    assert!(
        std::mem::offset_of!(Platform, zero_shader)
            == std::mem::offset_of!(NativePlatform, zero_shader)
    );
    assert!(
        std::mem::offset_of!(Platform, depth_state)
            == std::mem::offset_of!(NativePlatform, depth_state)
    );
    assert!(
        std::mem::offset_of!(Platform, texture) == std::mem::offset_of!(NativePlatform, texture)
    );
    assert!(std::mem::offset_of!(Platform, view) == std::mem::offset_of!(NativePlatform, view));
    assert!(std::mem::offset_of!(Platform, upload) == std::mem::offset_of!(NativePlatform, upload));
    assert!(std::mem::offset_of!(Platform, bind) == std::mem::offset_of!(NativePlatform, bind));
};

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
    fn new(
        platform: &Platform,
        pointer: Raw,
        runtime: Option<ConstantOps>,
        log: Log,
        user: Raw,
    ) -> Option<Self> {
        let release = platform.release;
        let device = Lease::new(pointer, release)?;
        unsafe { (platform.retain)(pointer) };
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
                let handle = unsafe { (platform.buffer)(pointer, size as u32) };
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
        let shader = unsafe { (platform.zero_shader)(pointer, log, user) };
        let zero_shader = Lease::new(shader, release)?;
        let depth = |equal| {
            let handle = unsafe { (platform.depth_state)(pointer, equal) };
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

    fn ensure_target(&mut self, platform: &Platform, extent: [u32; 2]) -> Option<i32> {
        if self.target.as_ref().is_some_and(|t| t.extent == extent) {
            return Some(RSF_MOTION_READY);
        }
        self.target = None;
        let device = self.device.as_ptr();
        let release = platform.release;
        let texture = unsafe { (platform.texture)(device, extent[0], extent[1]) };
        let texture = Lease::new(texture, release)?;
        let view = unsafe { (platform.view)(device, texture.as_ptr()) };
        self.target = Some(Target {
            view: Lease::new(view, release)?,
            texture,
            extent,
        });
        Some(RSF_MOTION_CREATED)
    }
}

pub(crate) struct Owner {
    platform: &'static Platform,
    device: Option<Device>,
    uploads: u64,
    upload_skips: u64,
}

impl Owner {
    pub(crate) fn new(platform: &'static Platform) -> Self {
        Self {
            platform,
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
        self.device = Device::new(self.platform, device, runtime, log, user);
        self.device.is_some()
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_device_new(platform: &'static Platform) -> *mut Owner {
    Box::into_raw(Box::new(Owner::new(platform)))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_device_drop(owner: *mut Owner) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_device_create(
    owner: &mut Owner,
    device: *mut c_void,
    log: Log,
    user: *mut c_void,
) -> u32 {
    u32::from(owner.create(device, None, log, user))
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_motion_device_target(
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
pub(crate) extern "C" fn rsf_anomaly_motion_device_get(
    owner: Option<&Owner>,
    out: &mut MaybeUninit<View>,
) {
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
pub(crate) extern "C" fn rsf_anomaly_motion_device_upload(
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
        buffer.uploaded = if let Some(runtime) = device.runtime {
            let input = rsf_patch_bytes {
                data: data.cast(),
                size,
            };
            unsafe { (runtime.upload)(context, resource, input) == RSF_PATCH_OK }
        } else {
            unsafe { (owner.platform.upload)(context, resource, data.cast(), size) != 0 }
        };
        if !buffer.uploaded {
            return 0;
        }
        buffer.last.copy_from_slice(bytes);
        owner.uploads += 1;
    }
    if let Some(runtime) = device.runtime {
        u32::from(
            unsafe { (runtime.bind)(context, buffer.stage as u32, 13, resource) } == RSF_PATCH_OK,
        )
    } else {
        unsafe { (owner.platform.bind)(context, resource, buffer.stage) }
    }
}

#[cfg(test)]
mod tests;
