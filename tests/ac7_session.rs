// SPDX-License-Identifier: MIT
use crate::carrier::translucency_scale;
use std::ffi::c_int;

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_session_fixture() -> c_int {
    assert_eq!(translucency_scale(200, 0, false), (1.0, Some(2.0)));
    assert_eq!(translucency_scale(200, 0, true), (2.0, None));
    assert_eq!(translucency_scale(200, 10, true), (0.25, None));
    assert_eq!(translucency_scale(200, 500, true), (4.0, None));
    assert_eq!(translucency_scale(50, 0, false), (0.5, None));
    assert_eq!(translucency_scale(0, 0, false), (1.0, None));
    0
}
