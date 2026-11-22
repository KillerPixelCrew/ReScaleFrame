// SPDX-License-Identifier: MIT
#[path = "schedule_abi.rs"]
mod abi;

use abi::*;
use std::ffi::{CStr, c_char, c_void};

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_core_schedule(
    scene: *const c_void,
    run: rsf_anomaly_motion_core_case_fn,
) -> i32 {
    let Some(run) = run else { return 1 };
    for phase in [
        RSF_ANOMALY_MOTION_ROOTS,
        RSF_ANOMALY_MOTION_FOLIAGE_LAYOUTS,
        RSF_ANOMALY_MOTION_CORE_DRAW,
        RSF_ANOMALY_MOTION_TREE_DRAW,
        RSF_ANOMALY_MOTION_DETAIL_DRAW,
    ] {
        if !unsafe { run(scene, phase as u32) } {
            return 1;
        }
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_far_schedule(
    run: rsf_anomaly_motion_far_case_fn,
    pairs: &mut u32,
    lq: &mut u32,
    worst: &mut f64,
) -> i32 {
    let Some(run) = run else { return 1 };
    // Use the engine's cached SKIN modes, including detail only for SKIN_0..2.
    let low = [
        (c"deffer_model_bump.vs", c"deffer_base_bump.ps"),
        (c"deffer_model_bump.vs", c"deffer_base_aref_bump.ps"),
        (c"deffer_model_flat.vs", c"deffer_base_flat.ps"),
        (c"deffer_model_flat.vs", c"deffer_base_aref_flat.ps"),
    ];
    let high = [
        (c"deffer_model_bump-hq.vs", c"deffer_base_bump-hq.ps"),
        (c"deffer_model_bump-hq.vs", c"deffer_base_steep-hq.ps"),
        (c"deffer_model_bump-hq.vs", c"deffer_base_aref_bump-hq.ps"),
    ];
    let detail = [
        (c"deffer_model_bump_d-hq.vs", c"deffer_base_bump_d-hq.ps"),
        (c"deffer_model_bump_d-hq.vs", c"deffer_base_bump_db-hq.ps"),
    ];
    let mut draw = |skin, element: &CStr, (vertex, pixel): (&CStr, &CStr)| {
        let case = rsf_anomaly_motion_far_case {
            vertex: vertex.as_ptr(),
            pixel: pixel.as_ptr(),
            element: element.as_ptr(),
            skin,
        };
        let mut far = 0.;
        if !unsafe { run(&case, &mut far) } {
            return false;
        }
        *worst = worst.max(far);
        *lq += u32::from(element == c"LQ");
        *pairs += 1;
        true
    };
    for skin in 0..=4 {
        for pair in low {
            if !draw(skin, c"LQ", pair) {
                return 1;
            }
        }
        for pair in high {
            if !draw(skin, c"HQ", pair) {
                return 1;
            }
        }
    }
    for skin in 0..=2 {
        for pair in detail {
            if !draw(skin, c"HQ", pair) {
                return 1;
            }
        }
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_tess_schedule(
    run: rsf_anomaly_motion_tess_case_fn,
    include_hm: bool,
    diagnose: bool,
    create: *const c_char,
) -> i32 {
    let Some(run) = run else { return 1 };
    let pn = [c"TESS_PN".as_ptr()];
    let detail = [
        c"TESS_PN".as_ptr(),
        c"USE_TDETAIL".as_ptr(),
        c"USE_TDETAIL_BUMP".as_ptr(),
    ];
    let hm = [
        c"TESS_PN".as_ptr(),
        c"TESS_HM".as_ptr(),
        c"USE_TDETAIL".as_ptr(),
    ];
    let pn_cases = [
        (c"SKIN_0 TESS_PN", 0),
        (c"SKIN_1 TESS_PN", 1),
        (c"SKIN_2 TESS_PN", 2),
        (c"SKIN_3 TESS_PN", 3),
        (c"SKIN_4 TESS_PN", 4),
    ];
    let detail_cases = [
        (c"SKIN_2 TESS_PN+USE_TDETAIL+USE_TDETAIL_BUMP", 2),
        (c"SKIN_3 TESS_PN+USE_TDETAIL+USE_TDETAIL_BUMP", 3),
    ];
    let hm_cases = [
        (c"SKIN_1 TESS_PN+TESS_HM+USE_TDETAIL", 1),
        (c"SKIN_2 TESS_PN+TESS_HM+USE_TDETAIL", 2),
    ];
    let draw =
        |(name, skin): (&CStr, i32), defines: &[*const c_char], create: *const c_char, diagnose| {
            let case = rsf_anomaly_motion_tess_case {
                name: name.as_ptr(),
                skin,
                defines: defines.as_ptr(),
                define_count: defines.len(),
            };
            unsafe { run(&case, create, diagnose) }
        };
    if !create.is_null() {
        return i32::from(!draw(hm_cases[0], &hm, create, false));
    }
    for (cases, defines) in [
        (pn_cases.as_slice(), pn.as_slice()),
        (detail_cases.as_slice(), detail.as_slice()),
        (
            if include_hm { hm_cases.as_slice() } else { &[] },
            hm.as_slice(),
        ),
    ] {
        for &case in cases {
            if !draw(case, defines, std::ptr::null(), diagnose) && !diagnose {
                return 1;
            }
        }
    }
    0
}
