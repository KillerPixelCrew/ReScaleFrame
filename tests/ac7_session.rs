// SPDX-License-Identifier: MIT
use std::ffi::c_int;

unsafe extern "C" {
    fn rsf_ac7_carrier_translucency_scale(
        render: u32,
        target: u32,
        override_percent: u32,
        depth_conformed: c_int,
        depth_limited_request: *mut f32,
    ) -> f32;
    fn rsf_ac7_translucency_scale(render_percent: u32, target_percent: u32) -> f32;
}

fn check_startup() {
    let mut limited = 0.0;
    let mut scale = |render, target, override_percent, depth| {
        let value = unsafe {
            rsf_ac7_carrier_translucency_scale(
                render,
                target,
                override_percent,
                depth,
                &mut limited,
            )
        };
        (value, limited)
    };
    assert_eq!(scale(50, 100, 0, 0), (1.0, 2.0));
    assert_eq!(scale(50, 100, 0, 1), (2.0, 0.0));
    assert_eq!(scale(50, 100, 10, 1), (0.25, 0.0));
    assert_eq!(scale(50, 100, 500, 1), (4.0, 0.0));
}

fn check_geometry() {
    for percent in [100, 50, 67, 58, 34, 100, 50] {
        let scale = unsafe { rsf_ac7_translucency_scale(percent, 50) };
        let native_scale = unsafe { rsf_ac7_translucency_scale(percent, 100) };
        for output in [3840.0, 2160.0] {
            let scene = output * percent as f32 / 100.0;
            assert!((scene * scale - output * 0.5).abs() < 0.001);
            assert!((scene * native_scale - output).abs() < 0.001);
        }
    }
    assert_eq!(unsafe { rsf_ac7_translucency_scale(0, 50) }, 0.5);
    assert_eq!(unsafe { rsf_ac7_translucency_scale(50, 0) }, 1.0);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_session_fixture() -> c_int {
    check_startup();
    check_geometry();
    0
}
