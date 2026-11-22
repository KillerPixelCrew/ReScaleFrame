// SPDX-License-Identifier: MIT
use crate::abi::{
    RSF_SR_DLSS as DLSS, RSF_SR_FSR3 as FSR3, RSF_SR_FSR4 as FSR4, RSF_SR_NONE as NONE,
    RSF_SR_XESS as XESS,
};

unsafe extern "C" {
    fn rsf_gpu_default_sr_backend(vendor: u32, software: u32) -> u32;
    fn rsf_gpu_select_sr_backend(vendor: u32, software: u32, choices: u32) -> u32;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_gpu_policy() -> i32 {
    let all = (1 << DLSS) | (1 << FSR3) | (1 << FSR4) | (1 << XESS);
    let defaults = [
        (0x10de, 0, DLSS),
        (0x8086, 0, XESS),
        (0x1002, 0, FSR3),
        (0, 0, FSR3),
        (0x1414, 1, NONE),
        (0x10de, 1, NONE),
    ];
    let selections = [
        (0x8086, 0, all, XESS),
        (0x10de, 0, all, DLSS),
        (0x1002, 0, all, FSR3),
        (0x8086, 0, 1 << FSR3, FSR3),
        (0x1002, 0, 1 << XESS, XESS),
        (0x10de, 0, 1 << FSR4, NONE),
        (0x8086, 1, all, NONE),
    ];
    let ok = defaults
        .into_iter()
        .all(|(vendor, software, expected)| unsafe {
            rsf_gpu_default_sr_backend(vendor, software) == expected
        })
        && selections
            .into_iter()
            .all(|(vendor, software, choices, expected)| unsafe {
                rsf_gpu_select_sr_backend(vendor, software, choices) == expected
            });
    if ok {
        println!(
            "PASS: NVIDIA DLSS, Intel XeSS, AMD/unknown FSR3, software disabled, choice fallback."
        );
    } else {
        eprintln!("FAIL: default or fallback SR policy");
    }
    i32::from(!ok)
}
