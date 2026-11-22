// SPDX-License-Identifier: MIT
//! The case lists of the far and tessellation-chain fixtures.

/// One G-buffer VS/PS pair X-Ray draws a skinned model with, on one side of r__dtex_range.
pub(super) struct FarCase {
    pub(super) vertex: &'static str,
    pub(super) pixel: &'static str,
    /// "LQ" (beyond r__dtex_range) or "HQ".
    pub(super) element: &'static str,
    pub(super) skin: i32,
}

/// Every cached SKIN mode of the LQ and HQ pairs, and the detail pairs only for SKIN_0..2.
pub(super) fn far_cases() -> Vec<FarCase> {
    let low = [
        ("deffer_model_bump.vs", "deffer_base_bump.ps"),
        ("deffer_model_bump.vs", "deffer_base_aref_bump.ps"),
        ("deffer_model_flat.vs", "deffer_base_flat.ps"),
        ("deffer_model_flat.vs", "deffer_base_aref_flat.ps"),
    ];
    let high = [
        ("deffer_model_bump-hq.vs", "deffer_base_bump-hq.ps"),
        ("deffer_model_bump-hq.vs", "deffer_base_steep-hq.ps"),
        ("deffer_model_bump-hq.vs", "deffer_base_aref_bump-hq.ps"),
    ];
    let detail = [
        ("deffer_model_bump_d-hq.vs", "deffer_base_bump_d-hq.ps"),
        ("deffer_model_bump_d-hq.vs", "deffer_base_bump_db-hq.ps"),
    ];
    let case = |skin, element, (vertex, pixel): (&'static str, &'static str)| FarCase {
        vertex,
        pixel,
        element,
        skin,
    };
    let mut cases = Vec::new();
    for skin in 0..=4 {
        cases.extend(low.iter().map(|&pair| case(skin, "LQ", pair)));
        cases.extend(high.iter().map(|&pair| case(skin, "HQ", pair)));
    }
    for skin in 0..=2 {
        cases.extend(detail.iter().map(|&pair| case(skin, "HQ", pair)));
    }
    cases
}

/// One tessellation permutation: what r_TessPass adds to all four stages.
pub(super) struct TessCase {
    pub(super) name: &'static str,
    pub(super) skin: i32,
    pub(super) defines: &'static [&'static str],
}

const PN: &[&str] = &["TESS_PN"];
const DETAIL: &[&str] = &["TESS_PN", "USE_TDETAIL", "USE_TDETAIL_BUMP"];
const HM: &[&str] = &["TESS_PN", "TESS_HM", "USE_TDETAIL"];

/// The HM cases stay opt-in because creating the stock HS can crash the driver.
pub(super) fn tess_cases(include_hm: bool) -> Vec<TessCase> {
    let mut cases = vec![
        TessCase {
            name: "SKIN_0 TESS_PN",
            skin: 0,
            defines: PN,
        },
        TessCase {
            name: "SKIN_1 TESS_PN",
            skin: 1,
            defines: PN,
        },
        TessCase {
            name: "SKIN_2 TESS_PN",
            skin: 2,
            defines: PN,
        },
        TessCase {
            name: "SKIN_3 TESS_PN",
            skin: 3,
            defines: PN,
        },
        TessCase {
            name: "SKIN_4 TESS_PN",
            skin: 4,
            defines: PN,
        },
        TessCase {
            name: "SKIN_2 TESS_PN+USE_TDETAIL+USE_TDETAIL_BUMP",
            skin: 2,
            defines: DETAIL,
        },
        TessCase {
            name: "SKIN_3 TESS_PN+USE_TDETAIL+USE_TDETAIL_BUMP",
            skin: 3,
            defines: DETAIL,
        },
    ];
    if include_hm {
        cases.extend(hm_cases());
    }
    cases
}

pub(super) fn hm_cases() -> [TessCase; 2] {
    [
        TessCase {
            name: "SKIN_1 TESS_PN+TESS_HM+USE_TDETAIL",
            skin: 1,
            defines: HM,
        },
        TessCase {
            name: "SKIN_2 TESS_PN+TESS_HM+USE_TDETAIL",
            skin: 2,
            defines: HM,
        },
    ]
}
