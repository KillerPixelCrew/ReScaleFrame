// SPDX-License-Identifier: MIT
use crate::lease::Lease;
use std::ffi::{c_char, c_void};

mod passes;
mod targets;
#[cfg(test)]
mod tests;

type Ptr = *mut c_void;
type Log = Option<unsafe extern "C" fn(Ptr, *const c_char)>;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Extent {
    width: u32,
    height: u32,
}

impl Extent {
    fn valid(self) -> bool {
        self.width != 0 && self.height != 0
    }

    fn fits(self, output: Self) -> bool {
        self.valid() && self.width <= output.width && self.height <= output.height
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Texture {
    size: Extent,
    mips: u32,
    layers: u32,
    format: u32,
    samples: u32,
    sample_quality: u32,
    usage: u32,
    bindings: u32,
    cpu_access: u32,
    misc: u32,
}

#[repr(C)]
#[derive(Default)]
struct View {
    texture2d: u32,
    format: u32,
    flags: u32,
    descriptor: [u32; 6],
}

#[repr(C)]
struct Draw {
    vertex: Ptr,
    pixel: Ptr,
    constants: Ptr,
    raster: Ptr,
    blend: Ptr,
    depth: Ptr,
    size: Extent,
}

#[repr(C)]
struct Platform {
    retain: unsafe extern "C" fn(Ptr),
    release: unsafe extern "C" fn(Ptr),
    as_texture: unsafe extern "C" fn(Ptr) -> Ptr,
    view_resource: unsafe extern "C" fn(Ptr) -> Ptr,
    texture_device: unsafe extern "C" fn(Ptr) -> Ptr,
    describe_texture: unsafe extern "C" fn(Ptr, &mut Texture),
    describe_view: unsafe extern "C" fn(Ptr, u32, &mut View),
    format_support: unsafe extern "C" fn(Ptr, u32) -> u32,
    texture: unsafe extern "C" fn(Ptr, &Texture, *const c_void, u32) -> Ptr,
    view: unsafe extern "C" fn(Ptr, Ptr, u32, Option<&View>) -> Ptr,
    shader: unsafe extern "C" fn(Ptr, u32, u32, *const c_char, Log, Ptr, &mut Ptr) -> i32,
    constant_buffer: unsafe extern "C" fn(Ptr) -> Ptr,
    raster_state: unsafe extern "C" fn(Ptr) -> Ptr,
    blend_state: unsafe extern "C" fn(Ptr) -> Ptr,
    depth_state: unsafe extern "C" fn(Ptr, u32) -> Ptr,
    save_state: unsafe extern "C" fn(Ptr) -> Ptr,
    restore_state: unsafe extern "C" fn(Ptr, Ptr),
    update_constants: unsafe extern "C" fn(Ptr, Ptr, *const u32),
    ps_constant: unsafe extern "C" fn(Ptr, u32, Ptr),
    targets: unsafe extern "C" fn(Ptr, u32, *const Ptr, Ptr),
    ps_resources: unsafe extern "C" fn(Ptr, u32, u32, *const Ptr),
    clear_stencil: unsafe extern "C" fn(Ptr, Ptr),
    draw: unsafe extern "C" fn(Ptr, &Draw),
}

const _: () = {
    assert!(size_of::<Extent>() == 8);
    assert!(size_of::<Texture>() == 44);
    assert!(size_of::<View>() == 36);
    assert!(size_of::<Draw>() == 56);
    assert!(size_of::<Platform>() == 184);
};

impl Platform {
    fn lease(&self, reference: Ptr) -> Option<Lease> {
        Lease::new(reference, self.release)
    }
}

fn pointer(lease: &Option<Lease>) -> Ptr {
    lease.as_ref().map_or(std::ptr::null_mut(), Lease::as_ptr)
}

// These values are checked against the SDK in the native leaves.
const RTV: u32 = 0;
const SRV: u32 = 1;
const UAV: u32 = 2;
const DSV: u32 = 3;
const DEPTH_SRV: u32 = 4;
const ROUND_TRIP: u32 = 0;
const UPSCALE: u32 = 1;
const PREPARE: u32 = 2;
const REINSERT: u32 = 3;
