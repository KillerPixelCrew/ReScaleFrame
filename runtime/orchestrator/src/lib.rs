// SPDX-License-Identifier: MIT

// Public backend IDs from rescaleframe/sr_session.h, checked by the native ABI test.
const NONE: u32 = 0;
const DLSS: u32 = 1;
const FSR3: u32 = 3;
const XESS: u32 = 5;

#[unsafe(no_mangle)]
pub extern "C" fn rsf_core_select_sr_backend(vendor: u32, software: u32, choices: u32) -> u32 {
    if software != 0 {
        return NONE;
    }
    let preferred = match vendor {
        0x10de => DLSS,
        0x8086 => XESS,
        _ => FSR3,
    };
    [preferred, FSR3, XESS]
        .into_iter()
        .find(|backend| choices & (1 << backend) != 0)
        .unwrap_or(NONE)
}
