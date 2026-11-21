// SPDX-License-Identifier: MIT
use super::{Raw, View};
use std::mem::{MaybeUninit, align_of};
use std::ptr::{drop_in_place, null, null_mut};

pub(crate) const REPLAY_READY: u8 = 0;
pub(crate) const REPLAY_NO_DEPTH_WRITE: u8 = 1;
const FAILED: u8 = 2;
const RENDER_TARGETS: usize = 8;
const CLASS_INTERFACES: usize = 253;

#[repr(C)]
#[derive(Default)]
struct Depth {
    targets: [Raw; RENDER_TARGETS],
    view: Raw,
    state: Raw,
    reference: u32,
    enabled: bool,
    writable: bool,
}

#[repr(C)]
struct Pixel {
    blend: Raw,
    factor: [f32; 4],
    mask: u32,
    shader: Raw,
    classes: [Raw; CLASS_INTERFACES],
    count: u32,
}

impl Default for Pixel {
    fn default() -> Self {
        Self {
            blend: null_mut(),
            factor: [0.0; 4],
            mask: 0,
            shader: null_mut(),
            classes: [null_mut(); CLASS_INTERFACES],
            count: 0,
        }
    }
}

#[repr(C)]
pub(crate) struct Platform {
    release: unsafe extern "C" fn(Raw),
    read_depth: unsafe extern "C" fn(Raw, &mut Depth),
    read_pixel: unsafe extern "C" fn(Raw, &mut Pixel),
    targets: unsafe extern "C" fn(Raw, u32, *const Raw, Raw),
    depth: unsafe extern "C" fn(Raw, Raw, u32),
    blend: unsafe extern "C" fn(Raw, Raw, *const f32, u32),
    shader: unsafe extern "C" fn(Raw, Raw, *const Raw, u32),
}

const _: () = {
    assert!(size_of::<Depth>() == 88);
    assert!(size_of::<Pixel>() == 2072);
    assert!(std::mem::offset_of!(Pixel, classes) == 40);
    assert!(size_of::<Platform>() == 56);
};

// Get calls return owned references. The scope drops them even when a
// read-only depth draw or a missing replay resource prevents rebinding.
struct Saved {
    platform: &'static Platform,
    context: Raw,
    depth: Depth,
    pixel: Pixel,
    modified: bool,
}

impl Drop for Saved {
    fn drop(&mut self) {
        let p = self.platform;
        let ctx = self.context;
        if self.modified {
            unsafe {
                (p.targets)(
                    ctx,
                    RENDER_TARGETS as u32,
                    self.depth.targets.as_ptr(),
                    self.depth.view,
                );
                (p.depth)(ctx, self.depth.state, self.depth.reference);
                (p.blend)(
                    ctx,
                    self.pixel.blend,
                    self.pixel.factor.as_ptr(),
                    self.pixel.mask,
                );
                (p.shader)(
                    ctx,
                    self.pixel.shader,
                    self.pixel.classes.as_ptr(),
                    self.pixel.count,
                );
            }
        }
        let count = (self.pixel.count as usize).min(CLASS_INTERFACES);
        for object in self
            .depth
            .targets
            .into_iter()
            .chain(self.pixel.classes[..count].iter().copied())
            .chain([
                self.depth.view,
                self.depth.state,
                self.pixel.blend,
                self.pixel.shader,
            ])
            .filter(|object| !object.is_null())
        {
            unsafe { (p.release)(object) };
        }
    }
}

#[repr(C)]
pub(crate) struct ReplayScope([MaybeUninit<u64>; 273]);

const _: () = {
    assert!(size_of::<Saved>() <= size_of::<ReplayScope>());
    assert!(align_of::<Saved>() <= align_of::<ReplayScope>());
};

/// A successful begin moves the saved state into the native frame.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_anomaly_motion_replay_begin(
    platform: &'static Platform,
    view: &View,
    context: Raw,
    scope: *mut ReplayScope,
) -> u8 {
    if context.is_null() || view.rtv.is_null() || view.zero_shader.is_null() {
        return FAILED;
    }
    let mut saved = Saved {
        platform,
        context,
        depth: Depth::default(),
        pixel: Pixel::default(),
        modified: false,
    };
    unsafe { (platform.read_depth)(context, &mut saved.depth) };
    let depth_state = if !saved.depth.view.is_null() && saved.depth.enabled {
        if !saved.depth.writable {
            return REPLAY_NO_DEPTH_WRITE;
        }
        view.equal_depth
    } else {
        view.no_depth
    };
    if depth_state.is_null() {
        return FAILED;
    }
    unsafe { (platform.read_pixel)(context, &mut saved.pixel) };
    if saved.pixel.count as usize > CLASS_INTERFACES {
        return FAILED;
    }
    let targets = [null_mut(), null_mut(), view.rtv];
    saved.modified = true;
    unsafe {
        (platform.targets)(
            context,
            targets.len() as u32,
            targets.as_ptr(),
            saved.depth.view,
        );
        (platform.depth)(context, depth_state, 0);
        (platform.blend)(context, null_mut(), null(), u32::MAX);
        (platform.shader)(context, view.zero_shader, null(), 0);
        scope.cast::<Saved>().write(saved);
    }
    REPLAY_READY
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_anomaly_motion_replay_end(scope: *mut ReplayScope) {
    unsafe { drop_in_place(scope.cast::<Saved>()) };
}
