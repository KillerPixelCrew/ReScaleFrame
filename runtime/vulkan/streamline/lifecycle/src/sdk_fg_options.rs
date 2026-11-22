// SPDX-License-Identifier: MIT

pub use crate::sys::rsf_slvk_sdk_fg_options as Packed;
#[cfg(windows)]
#[allow(unused_imports)] // The Vulkan Runtime consumes its own present timing sample.
pub use crate::sys::rsf_slvk_sdk_fg_sample as Sample;
#[cfg(windows)]
pub use crate::sys::rsf_slvk_sdk_fg_state as State;
use crate::sys::rsf_slvk_sdk_header;

/// An empty `sl::DLSSGState` v4 for slDLSSGGetState to fill.
#[cfg(windows)]
pub fn state() -> State {
    State {
        base: rsf_slvk_sdk_header {
            next: std::ptr::null_mut(),
            type_lo: 0xcc8a_c8e1,
            type_mid: 0xa179,
            type_hi: 0x44f5,
            type_tail: [0x97, 0xfa, 0xe7, 0x41, 0x12, 0xf9, 0xbc, 0x61],
            version: 4,
        },
        ..Default::default()
    }
}

pub fn pack(
    count: Option<u32>,
    width: u32,
    height: u32,
    format: u32,
    buffers: u32,
) -> Packed {
    Packed {
        base: rsf_slvk_sdk_header {
            next: std::ptr::null_mut(),
            type_lo: 0xfac5_f1cb,
            type_mid: 0x2dfd,
            type_hi: 0x4f36,
            type_tail: [0xa1, 0xe6, 0x3a, 0x9e, 0x86, 0x52, 0x56, 0xc5],
            version: 5,
        },
        mode: u32::from(count.is_some()),
        generated: count.unwrap_or(1),
        buffers,
        color_width: width,
        color_height: height,
        color_format: format,
        reserved_boolean: 2, // sl::Boolean::eInvalid
        ..Default::default()
    }
}
