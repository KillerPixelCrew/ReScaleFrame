// SPDX-License-Identifier: MIT
//! C layout of rsf_dump_pixels in rescaleframe/texture_dump.h.
use std::ffi::c_void;

#[repr(C)]
pub struct Pixels {
    pub struct_size: u32,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub row_pitch: u32,
    pub data_size: u64,
    pub data: *const c_void,
}

const _: () = {
    assert!(size_of::<Pixels>() == 40);
    assert!(std::mem::offset_of!(Pixels, row_pitch) == 16);
    assert!(std::mem::offset_of!(Pixels, data_size) == 24);
    assert!(std::mem::offset_of!(Pixels, data) == 32);
};
