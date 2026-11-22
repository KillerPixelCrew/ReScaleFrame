// SPDX-License-Identifier: MIT
use super::*;

fn selected(root: &Path, source: u64, native: u64) -> std::path::PathBuf {
    bundles(root)
        .find(|bundle| {
            let json = fs::read_to_string(bundle.join("engine_pass.json")).unwrap_or_default();
            json.contains(&format!("\"source_frame_id\":{source},"))
                && json.contains(&format!("\"native_frame\":{native},"))
        })
        .expect("selected engine pass")
}

fn depth_domains(pass: &mut rsf_game_render_pass) {
    let domains = &mut pass.depth_domains;
    domains.struct_size = size_of_val(domains) as u32;
    domains.version = RSF_DEPTH_DOMAINS_VERSION;
    domains.domain_count = 2;
    domains.clear_enabled = 1;
    domains.clear_value = 1.0;
    for (index, domain) in domains.domains.iter_mut().enumerate() {
        let (lo, hi, sign) = if index == 0 {
            (0.0, 0.25, 1.0)
        } else {
            (0.25, 1.0, -1.0)
        };
        domain.classify_min = lo;
        domain.classify_max = hi;
        domain.endpoint_flags = RSF_DEPTH_DOMAIN_INCLUDE_MIN;
        if index == 1 {
            domain.endpoint_flags |= RSF_DEPTH_DOMAIN_INCLUDE_MAX;
        }
        domain.viewport_xywh = [1.25 + index as f32, 0.75, 959.875, 539.75];
        domain.viewport_min_depth = lo;
        domain.viewport_max_depth = hi;
        domain.source_clip_to_target_clip = std::array::from_fn(|j| (j + 1) as f32 * sign * 0.125);
    }
}

pub(super) fn inspect(root: &Path, context: *mut c_void, ui: *mut c_void) {
    let mut generic = rsf_frame_record {
        struct_size: size_of::<rsf_frame_record>() as u32,
        abi_version: RSF_GAME_FRAME_ABI_VERSION,
        session_id: 29,
        frame_id: 30,
        view_id: 31,
        resource_generation: 2,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe {
        rsf_input_dump_request();
        assert_ne!(rsf_input_dump_begin_record(&generic, 2), 0);
        for mismatched in [
            rsf_frame_record {
                resource_generation: 3,
                ..generic
            },
            rsf_frame_record {
                abi_version: RSF_GAME_FRAME_ABI_VERSION + 1,
                ..generic
            },
        ] {
            assert_eq!(rsf_input_dump_wants(&mismatched), 0);
        }
        assert_ne!(
            rsf_input_dump_texture_record(
                context,
                &generic,
                c"final_color".as_ptr(),
                c"Generic D3D11 final".as_ptr(),
                ui,
                RAW,
                1.0
            ),
            0
        );
        rsf_input_dump_complete(&generic, 0);
        rsf_input_dump_request();
        generic.frame_id += 1;
        generic.flags = RSF_FRAME_FLAG_AMBIGUOUS_ID | RSF_FRAME_FLAG_NO_FG;
        assert_ne!(rsf_input_dump_begin_record(&generic, 1), 0);
        let render_only = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            role: RSF_GAME_RENDER_SR,
            session_id: generic.session_id,
            native_frame: generic.frame_id,
            view_key: generic.view_id.into(),
            flags: RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_NO_FG,
            ..std::mem::zeroed()
        };
        rsf_input_dump_pass_metadata(&render_only);
        rsf_input_dump_result(&generic, c"sr".as_ptr(), 1, 0);
        let sr = rsf_sr_frame {
            struct_size: size_of::<rsf_sr_frame>() as u32,
            record: &generic,
            ..Default::default()
        };
        rsf_input_dump_sr_frame(&sr, 1, 0);
        let bundle = selected(root, 0, generic.frame_id);
        for file in ["sr_status.json", "sr_parameters.json"] {
            let evidence = contents(&bundle, file);
            assert!(evidence.contains("\"source_frame_id\":null"));
            assert!(evidence.contains(&format!("\"frame_id\":{}", generic.frame_id)));
        }
        assert!(contents(&bundle, "engine_pass.json").contains(
            "\"depth_domains\":{\"requested\":0,\"struct_size\":0,\"version\":0,\"domain_count\":0"
        ));
        for wrong in [
            rsf_game_render_pass {
                native_frame: generic.frame_id + 1,
                ..render_only
            },
            rsf_game_render_pass {
                source_frame_id: generic.frame_id,
                ..render_only
            },
        ] {
            rsf_input_dump_pass(context, &wrong, 1);
            assert_eq!(rsf_input_dump_active(), 0);
        }
        rsf_input_dump_pass(context, &render_only, 1);
        assert_ne!(rsf_input_dump_active(), 0);
        rsf_input_dump_pass(context, &render_only, 0);
        rsf_input_dump_cancel(generic.session_id);

        rsf_input_dump_request();
        generic.frame_id += 1;
        generic.flags = RSF_FRAME_FLAG_NO_FG;
        assert_ne!(rsf_input_dump_begin_record(&generic, 1), 0);
        let mut source = rsf_game_render_pass {
            source_frame_id: generic.frame_id,
            native_frame: 12001,
            ..render_only
        };
        depth_domains(&mut source);
        rsf_input_dump_pass_metadata(&source);
        let bundle = selected(root, source.source_frame_id, source.native_frame);
        let original = contents(&bundle, "engine_pass.json");
        let expected = format!(
            "\"requested\":1,\"struct_size\":{},\"version\":1,\"domain_count\":2,\"clear_enabled\":1,\"clear_value\":1,\"reserved\":[0,0,0]",
            size_of_val(&source.depth_domains)
        );
        assert!(original.contains(&expected));
        for expected in [
            "\"classify_min\":0,\"classify_max\":0.25,\"endpoint_flags\":1",
            "\"classify_min\":0.25,\"classify_max\":1,\"endpoint_flags\":3",
            "\"viewport_xywh\":[1.25,0.75,959.875,539.75],\"viewport_min_depth\":0,\"viewport_max_depth\":0.25",
            "\"viewport_xywh\":[2.25,0.75,959.875,539.75],\"viewport_min_depth\":0.25,\"viewport_max_depth\":1",
            "\"source_clip_to_target_clip\":[0.125,0.25,0.375,0.5,0.625,0.75,0.875,1,1.125,1.25,1.375,1.5,1.625,1.75,1.875,2]",
            "\"source_clip_to_target_clip\":[-0.125,-0.25,-0.375,-0.5,-0.625,-0.75,-0.875,-1,-1.125,-1.25,-1.375,-1.5,-1.625,-1.75,-1.875,-2]",
        ] {
            assert!(original.contains(expected));
        }
        let mut wrong = rsf_game_render_pass {
            native_frame: 12002,
            ..source
        };
        wrong.depth_domains.domains[0].source_clip_to_target_clip[0] = 999.0;
        let short = rsf_game_render_pass {
            struct_size: source.struct_size - 1,
            ..source
        };
        for stale in [&wrong, &short] {
            rsf_input_dump_pass_metadata(stale);
            assert_eq!(contents(&bundle, "engine_pass.json"), original);
        }
        let mut nonfinite = rsf_game_render_pass { ..source };
        nonfinite.depth_domains.clear_value = f32::INFINITY;
        nonfinite.depth_domains.domains[1].source_clip_to_target_clip[15] = f32::NAN;
        nonfinite.depth_domains.domain_count = u32::MAX;
        rsf_input_dump_pass_metadata(&nonfinite);
        let bounded = contents(&bundle, "engine_pass.json");
        for expected in [
            "\"domain_count\":4294967295",
            "\"clear_value\":null",
            "-1.875,null]",
        ] {
            assert!(bounded.contains(expected));
        }
        rsf_input_dump_pass_metadata(&source);
        let fake_context = ptr::without_provenance_mut(1);
        rsf_input_dump_pass(fake_context, &wrong, 1);
        assert_eq!(rsf_input_dump_active(), 0);
        rsf_input_dump_pass(context, &source, 1);
        assert_ne!(rsf_input_dump_active(), 0);
        rsf_input_dump_pass(context, &source, 0);
        rsf_input_dump_cancel(generic.session_id);

        rsf_input_dump_request();
        generic.flags = 0;
        generic.frame_id += 1;
        assert_ne!(rsf_input_dump_begin_record(&generic, 2), 0);
        let ticket = rsf_input_dump_reserve(
            &generic,
            c"pending".as_ptr(),
            c"unsubmitted readback".as_ptr(),
            RAW,
            1.0,
        );
        assert_ne!(ticket, 0);
        assert_eq!(rsf_input_dump_gpu_submission(&generic), 0);
        rsf_input_dump_gpu_completed((ticket << 8) | 1, 3, fake_context);
        assert_eq!(
            rsf_input_dump_gpu_ready(ticket, c"pending".as_ptr(), 3, fake_context),
            0
        );
        rsf_input_dump_cancel(generic.session_id + 1);
        assert_ne!(rsf_input_dump_requested(), 0);
        rsf_input_dump_cancel(generic.session_id);
        assert_eq!(rsf_input_dump_requested(), 0);
        let stale = rsf_dump_pixels {
            struct_size: size_of::<rsf_dump_pixels>() as u32,
            ..Default::default()
        };
        assert_eq!(
            rsf_input_dump_pixels(ticket, c"pending".as_ptr(), &stale, 1, 0),
            0
        );
    }
}
