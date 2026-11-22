// SPDX-License-Identifier: MIT
//! Exact Anomaly 1.5.3 DX11 geometry profiles shared by admission, the
//! native hook leaf and the generated native projection.
//! Evidence: docs/research/anomaly-geometry-coverage.md, anomaly-avx-profile.md
//! and tools/anomaly_geometry_evidence.py.
use crate::engine_guards::{NativeSite, UniqueGuard, site};

pub(crate) const SITE_COUNT: usize = 17;
pub(crate) const LAYOUT_COUNT: usize = 7;
pub(crate) const UNIQUE_COUNT: usize = 9;

pub(crate) struct Profile {
    pub(crate) sites: [NativeSite; SITE_COUNT],
    pub(crate) layout_sites: [NativeSite; LAYOUT_COUNT],
    pub(crate) unique: [UniqueGuard; UNIQUE_COUNT],
    pub(crate) render_phase_rva: u32,
    pub(crate) second_viewport_rva: u32,
    pub(crate) world_matrix_rva: u32,
    pub(crate) vertex_constant_buffers_rva: u32,
    pub(crate) hierarchy_vtables: [u32; 4],
}

// SAFETY: the only raw pointers in the immutable records point to static C
// string literals. The records are never written through those pointers.
unsafe impl Sync for Profile {}

/// Indexed like the native build profiles: Standard, then AVX.
pub(crate) static PROFILES: [Profile; 2] = [
    // ---- Standard 1.5.3 DX11 ----
    Profile {
        sites: [
            site(
                c"CRender::model_Create",
                0x839940,
                b"488b8958070000e914e20400cccccccc",
            ),
            site(
                c"CRender::model_CreateChild",
                0x839950,
                b"488b8958070000e974e40400cccccccc",
            ),
            site(
                c"CRender::model_Duplicate",
                0x839bd0,
                b"488b8958070000e9d4ea0400cccccccc",
            ),
            site(
                c"CRender::model_Delete",
                0x839b40,
                b"40534883ec20488b02488bda488b8958",
            ),
            site(
                c"Fvisual::Render",
                0x8cff60,
                b"48895c2410555657415641574883ec30",
            ),
            site(
                c"FProgressive::Render",
                0x8d0800,
                b"48895c241048896c2420565741544156",
            ),
            site(
                c"CSkeletonX_ST::Render",
                0x8d1610,
                b"4883ec384c8bc1488d51684881c1a000",
            ),
            site(
                c"CSkeletonX_PM::Render",
                0x8d1520,
                b"4883ec38448b89c80000000f57c00f2f",
            ),
            site(
                c"FTreeVisual_ST::Render",
                0x8d5d20,
                b"48895c2410555657415641574883ec30",
            ),
            site(
                c"FTreeVisual_PM::Render",
                0x8d5a40,
                b"48895c241848896c2420565741544156",
            ),
            site(
                c"CBackend::constants.flush",
                0x84b930,
                b"48895c2408574883ec20488d1da78551",
            ),
            site(
                c"R_dsgraph_structure::r_dsgraph_render_hud",
                0x84ffa0,
                b"488bc448897010574881ec500100000f",
            ),
            site(
                c"R_dsgraph_structure::r_dsgraph_render_sorted",
                0x850cc0,
                b"488bc44889580848897010574881ec10",
            ),
            site(
                c"R_dsgraph_structure::r_dsgraph_render_emissive",
                0x84ec80,
                b"488bc44889580848897010574881ec10",
            ),
            site(
                c"CRender::rmNear",
                0x83a450,
                b"40534883ec40488b01ff507848c74424",
            ),
            site(
                c"CRender::rmNormal",
                0x83a4d0,
                b"40534883ec40488b01ff507848c74424",
            ),
            site(
                c"CDetailManager::Render",
                0x86a8c0,
                b"40534883ec204883b95804000000488b",
            ),
        ],
        layout_sites: [
            site(
                c"R_xforms::set_W",
                0x840960,
                b"48895c240848896c2410488974241857",
            ),
            site(
                c"CKinematics::LL_GetBoneInstance",
                0x803d90,
                b"0fb7c24869c0a800000048034150c3cc",
            ),
            site(
                c"CKinematics::LL_GetTransform_R",
                0x803da0,
                b"4883ec28488b01ff50484883c0404883",
            ),
            site(
                c"Fvisual::Render phase compare",
                0x8cff83,
                b"833d06504900010f850b020000813d0a",
            ),
            site(
                c"CSecondVPParams::IsSVPFrame",
                0x2d260,
                b"80390074150fb6490133d28b050fa4d0",
            ),
            site(
                c"dx10ConstantBuffer::Flush",
                0x83de40,
                b"40534883ec4080796800488bd9743248",
            ),
            site(
                c"FTreeVisual_PM last_lod load",
                0x8d5a66,
                b"448b8b080100000f57c00f2ff00f8291",
            ),
        ],
        unique: [
            UniqueGuard::new(
                c"Fvisual::Render",
                0x8cff60,
                c"48895c2410555657415641574883ec30488bb9",
            ),
            UniqueGuard::new(
                c"FProgressive::Render",
                0x8d0800,
                c"48895c241048896c242056574154415641574883ec30",
            ),
            UniqueGuard::new(
                c"FTreeVisual_ST::Render",
                0x8d5d20,
                c"48895c2410555657415641574883ec30488bd9",
            ),
            UniqueGuard::new(
                c"FTreeVisual_PM::Render",
                0x8d5a40,
                c"48895c241848896c242056574154415641574883ec40",
            ),
            UniqueGuard::new(
                c"R_dsgraph_structure::r_dsgraph_render_sorted",
                0x850cc0,
                c"488bc44889580848897010574881ec1001000083b9e8",
            ),
            UniqueGuard::new(
                c"R_dsgraph_structure::r_dsgraph_render_emissive",
                0x84ec80,
                c"488bc44889580848897010574881ec1001000083b958",
            ),
            UniqueGuard::new(
                c"CRender::rmNear",
                0x83a450,
                c"40534883ec40488b01ff507848c744242000000000488bc8488bd8488b10ff52508bc80f57c0f3480f2ac1488bcbf30f11442428488b13ff5258488b0d47",
            ),
            UniqueGuard::new(
                c"CRender::rmNormal",
                0x83a4d0,
                c"40534883ec40488b01ff507848c744242000000000488bc8488bd8488b10ff52508bc80f57c0f3480f2ac1488bcbf30f11442428488b13ff5258488b0dc7",
            ),
            UniqueGuard::new(
                c"R_xforms::set_W",
                0x840960,
                c"48895c240848896c24104889742418574883ec208b0248",
            ),
        ],
        render_phase_rva: 0xd64f90,
        second_viewport_rva: 0xd37b30,
        world_matrix_rva: 0xd63c18,
        vertex_constant_buffers_rva: 0xd63e78,
        hierarchy_vtables: [0xb67d20, 0xb64ce8, 0xb71548, 0xb6c460],
    },
    // ---- AVX 1.5.3 DX11 ----
    Profile {
        sites: [
            site(
                c"CRender::model_Create",
                0x830bf0,
                b"488b8958070000e944ce0400cccccccc",
            ),
            site(
                c"CRender::model_CreateChild",
                0x830c00,
                b"488b8958070000e9a4d00400cccccccc",
            ),
            site(
                c"CRender::model_Duplicate",
                0x830e80,
                b"488b8958070000e904d70400cccccccc",
            ),
            site(
                c"CRender::model_Delete",
                0x830df0,
                b"40534883ec20488b02488bda488b8958",
            ),
            site(
                c"Fvisual::Render",
                0x8c5970,
                b"48895c2410555657415641574883ec30",
            ),
            site(
                c"FProgressive::Render",
                0x8c6210,
                b"48895c241048896c2420565741544156",
            ),
            site(
                c"CSkeletonX_ST::Render",
                0x8c7010,
                b"4883ec384c8bc1488d51684881c1a000",
            ),
            site(
                c"CSkeletonX_PM::Render",
                0x8c6f20,
                b"4883ec38448b89c80000004c8bd9c5f8",
            ),
            site(
                c"FTreeVisual_ST::Render",
                0x8cb5e0,
                b"48895c2410555657415641574883ec30",
            ),
            site(
                c"FTreeVisual_PM::Render",
                0x8cb2f0,
                b"48895c241848896c2420565741544156",
            ),
            site(
                c"CBackend::constants.flush",
                0x842490,
                b"48895c2408574883ec20488d1d47fa50",
            ),
            site(
                c"R_dsgraph_structure::r_dsgraph_render_hud",
                0x846aa0,
                b"488bc448897010574881ec80010000c5",
            ),
            site(
                c"R_dsgraph_structure::r_dsgraph_render_sorted",
                0x8476a0,
                b"488bc44889580848897010574881ec40",
            ),
            site(
                c"R_dsgraph_structure::r_dsgraph_render_emissive",
                0x8457e0,
                b"488bc44889580848897010574881ec40",
            ),
            site(
                c"CRender::rmNear",
                0x831700,
                b"40534883ec50488b01c5f829742440ff",
            ),
            site(
                c"CRender::rmNormal",
                0x8317a0,
                b"40534883ec50488b01c5f829742440ff",
            ),
            site(
                c"CDetailManager::Render",
                0x860170,
                b"40534883ec204883b95804000000488b",
            ),
        ],
        layout_sites: [
            site(
                c"R_xforms::set_W",
                0x837c50,
                b"48895c240848896c2410488974241857",
            ),
            site(
                c"CKinematics::LL_GetBoneInstance",
                0x7fd6a0,
                b"0fb7c24869c0a800000048034150c3cc",
            ),
            site(
                c"CKinematics::LL_GetTransform_R",
                0x7fd6b0,
                b"4883ec28488b01ff50484883c0404883",
            ),
            site(
                c"Fvisual::Render phase compare",
                0x8c5993,
                b"833df6d54800010f850b020000813dfa",
            ),
            site(
                c"CSecondVPParams::IsSVPFrame",
                0x2d180,
                b"80390074150fb6490133d28b05ef84cf",
            ),
            site(
                c"dx10ConstantBuffer::Flush",
                0x835130,
                b"40534883ec4080796800488bd9743248",
            ),
            site(
                c"FTreeVisual_PM last_lod load",
                0x8cb318,
                b"448b8b08010000c5f857c0c5f82ff00f",
            ),
        ],
        unique: [
            UniqueGuard::new(
                c"Fvisual::Render",
                0x8c5970,
                c"48895c2410555657415641574883ec30488bb9",
            ),
            UniqueGuard::new(
                c"FProgressive::Render",
                0x8c6210,
                c"48895c241048896c242056574154415641574883ec30",
            ),
            UniqueGuard::new(
                c"FTreeVisual_ST::Render",
                0x8cb5e0,
                c"48895c2410555657415641574883ec30488bd9",
            ),
            UniqueGuard::new(
                c"FTreeVisual_PM::Render",
                0x8cb2f0,
                c"48895c241848896c242056574154415641574883ec40",
            ),
            UniqueGuard::new(
                c"R_dsgraph_structure::r_dsgraph_render_sorted",
                0x8476a0,
                c"488bc44889580848897010574881ec4001000083b9e8",
            ),
            UniqueGuard::new(
                c"R_dsgraph_structure::r_dsgraph_render_emissive",
                0x8457e0,
                c"488bc44889580848897010574881ec4001000083b958",
            ),
            UniqueGuard::new(
                c"CRender::rmNear",
                0x831700,
                c"40534883ec50488b01c5f829742440ff5078c5c857f6c5fa11742420c5fa11742424488b10488bc8488bd8ff52508bc8c5f857c0c4e1fa2ac1c5fa11442428488b13488bcbff5258488b0d89",
            ),
            UniqueGuard::new(
                c"CRender::rmNormal",
                0x8317a0,
                c"40534883ec50488b01c5f829742440ff5078c5c857f6c5fa11742420c5fa11742424488b10488bc8488bd8ff52508bc8c5f857c0c4e1fa2ac1c5fa11442428488b13488bcbff5258488b0de9",
            ),
            UniqueGuard::new(
                c"R_xforms::set_W",
                0x837c50,
                c"48895c240848896c24104889742418574883ec208b0248",
            ),
        ],
        render_phase_rva: 0xd52f90,
        second_viewport_rva: 0xd25b30,
        world_matrix_rva: 0xd51c18,
        vertex_constant_buffers_rva: 0xd51e78,
        hierarchy_vtables: [0xb54e90, 0xb51dd8, 0xb5e708, 0xb595d0],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_guards::decode_hex;
    use crate::native_profile;
    use std::ffi::CStr;

    fn name(site: &NativeSite) -> &str {
        unsafe { CStr::from_ptr(site.name) }.to_str().unwrap()
    }

    #[test]
    fn detour_order_is_the_native_slot_order() {
        const ORDER: [&str; SITE_COUNT] = [
            "CRender::model_Create",
            "CRender::model_CreateChild",
            "CRender::model_Duplicate",
            "CRender::model_Delete",
            "Fvisual::Render",
            "FProgressive::Render",
            "CSkeletonX_ST::Render",
            "CSkeletonX_PM::Render",
            "FTreeVisual_ST::Render",
            "FTreeVisual_PM::Render",
            "CBackend::constants.flush",
            "R_dsgraph_structure::r_dsgraph_render_hud",
            "R_dsgraph_structure::r_dsgraph_render_sorted",
            "R_dsgraph_structure::r_dsgraph_render_emissive",
            "CRender::rmNear",
            "CRender::rmNormal",
            "CDetailManager::Render",
        ];
        for profile in &PROFILES {
            let names: Vec<_> = profile.sites.iter().map(name).collect();
            assert_eq!(names, ORDER);
        }
    }

    #[test]
    fn unique_prefixes_extend_a_profiled_site() {
        for profile in &PROFILES {
            for guard in &profile.unique {
                let hex = unsafe { CStr::from_ptr(guard.expected_hex) }.to_bytes();
                let bytes = decode_hex(hex).unwrap();
                let site = profile
                    .sites
                    .iter()
                    .chain(&profile.layout_sites)
                    .find(|site| site.rva == guard.rva)
                    .expect("guard names a profiled site");
                assert!(bytes.len() > site.expected.len());
                assert_eq!(bytes[..site.expected.len()], site.expected);
            }
        }
    }

    #[test]
    fn every_fact_lies_inside_its_image() {
        for (geometry, native) in PROFILES.iter().zip(&native_profile::PROFILES) {
            let inside =
                |rva: u32, bytes: u32| rva.checked_add(bytes).unwrap() <= native.image_size;
            for site in geometry.sites.iter().chain(&geometry.layout_sites) {
                assert!(inside(site.rva, 16), "{}", name(site));
            }
            for guard in &geometry.unique {
                let hex = unsafe { CStr::from_ptr(guard.expected_hex) }.to_bytes();
                assert!(inside(guard.rva, (hex.len() / 2) as u32));
            }
            for rva in [
                geometry.render_phase_rva,
                geometry.second_viewport_rva,
                geometry.world_matrix_rva,
                geometry.vertex_constant_buffers_rva,
            ]
            .into_iter()
            .chain(geometry.hierarchy_vtables)
            {
                assert!(inside(rva, 8));
            }
            assert!(inside(native.device_frame_rva, 4));
        }
    }
}
