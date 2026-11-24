// SPDX-License-Identifier: MIT

use crate::sdk_api::header;
pub use crate::sys::rsf_slvk_sdk_fg_options as Packed;
#[cfg(windows)]
pub use crate::sys::rsf_slvk_sdk_fg_sample as Sample;
#[cfg(windows)]
pub use crate::sys::rsf_slvk_sdk_fg_state as State;

/// An empty `sl::DLSSGState` v4 for slDLSSGGetState to fill.
#[cfg(windows)]
pub fn state() -> State {
    State {
        base: header(
            0xcc8a_c8e1,
            0xa179,
            0x44f5,
            [0x97, 0xfa, 0xe7, 0x41, 0x12, 0xf9, 0xbc, 0x61],
            4,
        ),
        ..Default::default()
    }
}

pub fn pack(count: Option<u32>, width: u32, height: u32, format: u32, buffers: u32) -> Packed {
    Packed {
        base: header(
            0xfac5_f1cb,
            0x2dfd,
            0x4f36,
            [0xa1, 0xe6, 0x3a, 0x9e, 0x86, 0x52, 0x56, 0xc5],
            5,
        ),
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
