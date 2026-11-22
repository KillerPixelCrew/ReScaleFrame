// SPDX-License-Identifier: MIT
use crate::streamline_abi::*;
use std::{cell::Cell, ptr};

const AMD: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const NVIDIA: [u8; 8] = [8, 7, 6, 5, 4, 3, 2, 1];
const IDENTICAL_MODEL: [u8; 8] = [2, 2, 3, 4, 5, 6, 7, 8];

thread_local! {
    static MALFORMED: Cell<bool> = const { Cell::new(false) };
    static AMBIGUOUS: Cell<bool> = const { Cell::new(false) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_sl_find_adapter(
    physical: u32,
    luid: *const u8,
    length: u32,
    mask: u32,
) -> u32 {
    // Supplying a native physical device would enable upstream device-ID matching.
    if physical != 0 || luid.is_null() || length != 8 {
        MALFORMED.set(true);
        return 1;
    }
    let luid = unsafe { &*luid.cast::<[u8; 8]>() };
    let matched = [AMD, NVIDIA, IDENTICAL_MODEL]
        .iter()
        .enumerate()
        .any(|(index, adapter)| mask & (1 << index) != 0 && adapter == luid);
    if matched || (AMBIGUOUS.get() && mask == 8) {
        0
    } else {
        2
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_sl_selected_run() -> i32 {
    let identify = |luid: &[u8; 8]| unsafe { rsf_fixture_sl_selected_mask(luid.as_ptr(), 8, 1) };
    let amd = identify(&AMD);
    let nvidia = identify(&NVIDIA);
    let identical = identify(&IDENTICAL_MODEL);
    let features = [0, 1000, 3, 0xffff, 4, 0xfffe];
    let masks = [2, 2, 7, 7, 7, 0];
    let requirements = |bit, vendor| {
        features
            .into_iter()
            .zip(masks)
            .enumerate()
            .fold(0, |bits, (index, (feature, mask))| {
                let applies =
                    unsafe { rsf_fixture_sl_device_config_applies(feature, mask, bit, vendor) };
                bits | (applies << index)
            })
    };
    let unknown = [9, 0, 0, 0, 0, 0, 0, 0];
    let mut checks = [
        (
            (amd, nvidia, identical) == (1, 2, 4) && !MALFORMED.get(),
            "Selected LUID preserves adapter numbering and distinguishes identical models",
        ),
        (
            (requirements(amd, 0x1002), requirements(nvidia, 0x10de)) == (0x18, 0x1f),
            "AMD retains common/PCL and NVIDIA retains its supported requirements",
        ),
        (
            unsafe {
                rsf_fixture_sl_config_applies(7, 0) == 0
                    && rsf_fixture_sl_config_applies(7, 3) == 0
                    && rsf_fixture_sl_config_applies(0, 1) == 0
            },
            "Unknown, ambiguous and unsupported masks refuse",
        ),
        (
            identify(&unknown) == 0
                && unsafe {
                    rsf_fixture_sl_selected_mask(ptr::null(), 8, 1) == 0
                        && rsf_fixture_sl_selected_mask(AMD.as_ptr(), 7, 1) == 0
                        && rsf_fixture_sl_selected_mask(AMD.as_ptr(), 8, 0) == 0
                },
            "Missing identity or lookup refuses",
        ),
        (false, "Multiple matching adapter bits refuse"),
    ];
    AMBIGUOUS.set(true);
    checks[4].0 = identify(&AMD) == 0;
    AMBIGUOUS.set(false);
    let mut failed = false;
    for (passed, message) in checks {
        if !passed {
            eprintln!("{message}");
            failed = true;
        }
    }
    if !failed {
        println!("selected-device requirement filtering passed");
    }
    i32::from(failed)
}
