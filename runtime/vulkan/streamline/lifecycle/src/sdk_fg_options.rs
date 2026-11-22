// SPDX-License-Identifier: MIT
#[path = "sdk_fg_options_abi.rs"]
mod abi;

pub(crate) use abi::rsf_slvk_sdk_fg_options as Packed;
use abi::rsf_slvk_sdk_header;

pub(crate) fn pack(
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
