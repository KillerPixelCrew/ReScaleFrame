// SPDX-License-Identifier: MIT
use super::*;
use crate::engine_guards::NativeSite;
use rsf_game_sys::pe32_plus::Pe32Plus;
use rsf_game_sys::signature::{EXECUTE, Pattern};

fn parses(text: &CStr) -> bool {
    let text = text.to_str().unwrap();
    text.split('|')
        .all(|candidate| Pattern::parse(candidate).is_some())
}

#[test]
fn every_pattern_and_check_parses_and_every_site_is_named_once() {
    for entry in &SIGNATURES {
        assert!(parses(entry.pattern), "{:?}", entry.name);
        assert!(
            entry.check.is_empty() || parses(entry.check),
            "{:?}",
            entry.name
        );
    }
    for (_, name, check) in &DERIVED {
        assert!(check.is_empty() || parses(check), "{name:?}");
    }
    let mut names = (0..SITES).map(name).collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), SITES);
}

#[test]
fn a_refused_site_refuses_only_the_records_that_need_its_feature() {
    let mut resolved = Resolved {
        sites: [0; SITES],
        refused: Default::default(),
    };
    resolved.refuse(Site::PhaseBlur, "was not found");
    let read = |_: usize, _: &mut [u8]| false;
    let records = records_of(
        &resolved,
        &read,
        Identity {
            executable: c"AnomalyDX11.exe",
            sha256: c"",
            image_size: 0x1000,
        },
    );
    let refusal = c"Anomaly SR refused: site 'CRenderTarget::phase_blur' was not found.";
    assert_eq!(records.sr.err(), Some(refusal));
    assert_eq!(records.motion.err(), Some(refusal));
    assert!(records.native.is_ok() && records.geometry.is_ok() && records.input.is_ok());
}

/// The installed executables, `RSF_ANOMALY_DIR` naming another copy of the game.
fn installed() -> Vec<(usize, std::path::PathBuf)> {
    let root = std::env::var_os("RSF_ANOMALY_DIR")
        .map_or_else(|| "D:\\Anomaly 1.5.3".into(), std::path::PathBuf::from);
    ["AnomalyDX11.exe", "AnomalyDX11AVX.exe"]
        .into_iter()
        .enumerate()
        .map(|(build, name)| (build, root.join("bin").join(name)))
        .filter(|(_, path)| path.is_file())
        .collect()
}

fn same_sites(discovered: &[NativeSite], recorded: &[NativeSite], what: &str) {
    assert_eq!(discovered.len(), recorded.len(), "{what}");
    for (found, expected) in discovered.iter().zip(recorded) {
        let name = unsafe { CStr::from_ptr(expected.name) };
        assert_eq!(unsafe { CStr::from_ptr(found.name) }, name, "{what}");
        assert_eq!(found.rva, expected.rva, "{what} {name:?}");
        assert_eq!(found.expected, expected.expected, "{what} {name:?}");
    }
}

/// Every pattern resolves uniquely in both installed DX11 executables, and the records built
/// from the discovered sites equal the recorded ones. Skipped when the game is not installed.
#[test]
fn every_site_resolves_in_the_installed_executables() {
    let installed = installed();
    if installed.is_empty() {
        eprintln!("skipped: Anomaly 1.5.3 is not installed");
        return;
    }
    for (build, path) in installed {
        let bytes = std::fs::read(&path).expect("read the installed executable");
        let image = Pe32Plus::parse(&bytes).expect("PE32+ executable");
        let sections = image.sections(false);
        let code = sections
            .iter()
            .filter(|(_, _, flags, _)| flags & EXECUTE != 0)
            .map(|&(_, rva, _, bytes)| Section { rva, bytes })
            .collect::<Vec<_>>();
        // Headers precede the first section at the same offset in the file and the image.
        let headers = sections.iter().map(|section| section.1).min().unwrap_or(0);
        let read = |rva: usize, out: &mut [u8]| {
            image
                .span(rva, out.len())
                .map(|(bytes, _)| bytes)
                .or_else(|| bytes.get(rva..rva + out.len()).filter(|_| rva < headers))
                .map(|bytes| out.copy_from_slice(bytes))
                .is_some()
        };
        let resolved = resolve_image(&code, image.preferred_base(), &read);
        for (feature, refused) in FEATURE_NAMES.iter().zip(&resolved.refused) {
            assert!(
                refused.is_none(),
                "{feature} in {}: {refused:?}",
                path.display()
            );
        }
        let records = records_of(
            &resolved,
            &read,
            Identity {
                executable: c"",
                sha256: c"",
                image_size: image.image_size().unwrap(),
            },
        );
        let what = path.display().to_string();
        let (native, recorded) = (
            records.native.unwrap(),
            &crate::native_profile::RECORDED[build],
        );
        assert_eq!(native.image_size, recorded.image_size, "{what}");
        assert_eq!(
            [
                native.device_width_rva,
                native.device_height_rva,
                native.device_frame_rva
            ],
            [
                recorded.device_width_rva,
                recorded.device_height_rva,
                recorded.device_frame_rva
            ],
            "{what}"
        );
        same_sites(&native.sites, &recorded.sites, &what);

        let (geometry, recorded) = (
            records.geometry.unwrap(),
            &crate::geometry_profile::RECORDED[build],
        );
        same_sites(&geometry.sites, &recorded.sites, &what);
        same_sites(&geometry.layout_sites, &recorded.layout_sites, &what);
        assert_eq!(
            [
                geometry.render_phase_rva,
                geometry.second_viewport_rva,
                geometry.world_matrix_rva,
                geometry.vertex_constant_buffers_rva,
            ],
            [
                recorded.render_phase_rva,
                recorded.second_viewport_rva,
                recorded.world_matrix_rva,
                recorded.vertex_constant_buffers_rva,
            ],
            "{what}"
        );
        assert_eq!(
            geometry.hierarchy_vtables, recorded.hierarchy_vtables,
            "{what}"
        );

        let (sr, recorded) = (records.sr.unwrap(), &crate::sr_profile::RECORDED[build]);
        same_sites(&sr.function_sites, &recorded.function_sites, &what);
        same_sites(&sr.call_sites, &recorded.call_sites, &what);
        for (found, expected) in sr
            .first_person_sites
            .iter()
            .zip(&recorded.first_person_sites)
        {
            assert_eq!(found.name, expected.name, "{what}");
            assert_eq!(
                [found.function_rva, found.set_p_call, found.rm_near_call],
                [
                    expected.function_rva,
                    expected.set_p_call,
                    expected.rm_near_call
                ],
                "{what} {:?}",
                expected.name
            );
        }
        let globals = |profile: &SrProfile| {
            [
                profile.crender_vtable_rva,
                profile.device_view_rva,
                profile.device_project_rva,
                profile.device_full_transform_rva,
                profile.device_precache_frame_rva,
                profile.load_screen_registered_rva,
                profile.hw_device_rva,
                profile.hw_context_rva,
                profile.hw_swap_chain_rva,
                profile.hw_base_rt_rva,
                profile.hw_base_zb_rva,
                profile.render_implementation_rva,
                profile.render_options_word0_rva,
                profile.render_options_word3_rva,
                profile.sunshafts_mode_rva,
                profile.sampler_cache_rva,
                profile.user_mip_bias_rva,
                profile.user_anisotropy_rva,
                profile.ssao_set_viewport_rva,
                profile.ssao_viewport_rva,
                profile.ssao_viewport_guard_rva,
                profile.backend_rva,
                profile.position_setup_vtable_rva,
                profile.get_constant_buffer_rva,
            ]
        };
        assert_eq!(globals(sr), globals(recorded), "{what}");
        let guard_rvas =
            |guards: &[UniqueGuard]| guards.iter().map(|guard| guard.rva).collect::<Vec<_>>();
        assert_eq!(
            guard_rvas(&sr.reconstruction_guards),
            guard_rvas(&recorded.reconstruction_guards),
            "{what}"
        );
        assert!(
            guard_rvas(recorded.unique_guards)
                .iter()
                .all(|rva| guard_rvas(sr.unique_guards).contains(rva)),
            "{what}"
        );

        let (motion, recorded) = (
            records.motion.unwrap(),
            &crate::motion_profile::RECORDED[build],
        );
        same_sites(&motion.sites, &recorded.sites, &what);
        same_sites(
            &motion.create_shader_calls,
            &recorded.create_shader_calls,
            &what,
        );
        let rvas = |profile: &MotionProfile| {
            let frame = &profile.frame;
            let tess = &profile.tess_create_sites;
            [
                profile.shader_compile_rva,
                profile.d3dcompile_setup_rva,
                profile.source_name_rva,
                profile.tess_dispatch_rva,
                tess[0].rva,
                tess[0].wrapper_rva,
                tess[1].rva,
                tess[1].wrapper_rva,
                frame.source_offset,
                frame.length_offset,
                frame.entry_offset,
                frame.flags_offset,
                frame.defines_offset,
            ]
        };
        assert_eq!(rvas(motion), rvas(recorded), "{what}");

        let (input, recorded) = (records.input.unwrap(), &crate::input::RECORDED[build]);
        same_sites(&input.sites, &recorded.sites, &what);
        assert_eq!(input.slot_rva, recorded.slot_rva, "{what}");
    }
}
