// SPDX-License-Identifier: GPL-3.0-only
use super::{abi::*, bundles, contents, entry, root, utf8};
use rsf_game_sys::{
    RSF_FRAME_FLAG_AMBIGUOUS_ID, RSF_FRAME_FLAG_NO_FG, RSF_GAME_CAMERA_ABI_VERSION,
    RSF_GAME_FRAME_ABI_VERSION, RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_SR,
    RSF_GAME_RENDER_WINDOW, rsf_frame_record, rsf_game_render_pass,
};
use std::ffi::{c_char, c_void};
use std::{fs, path::Path, ptr};

unsafe extern "C" fn count(user: *mut c_void, _: *const c_char) {
    // A logging callback must be able to query the capture synchronously.
    unsafe {
        rsf_input_dump_requested();
        *user.cast::<u32>() += 1;
    }
}

fn run(path: &Path) -> i32 {
    let root = root(path, "capture-metadata");
    fs::create_dir_all(&root).unwrap();
    let root_utf8 = utf8(&root);
    let mut messages = 0u32;
    let mut frame = rsf_frame_record {
        struct_size: size_of::<rsf_frame_record>() as u32,
        abi_version: RSF_GAME_FRAME_ABI_VERSION,
        session_id: 7,
        frame_id: 11,
        view_id: 13,
        resource_generation: 2,
        render_width: 32,
        render_height: 32,
        output_width: 64,
        output_height: 64,
        ..unsafe { std::mem::zeroed() }
    };
    frame.camera.struct_size = size_of_val(&frame.camera) as u32;
    frame.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION + 1;
    let mut pass = rsf_game_render_pass {
        struct_size: size_of::<rsf_game_render_pass>() as u32,
        flags: RSF_GAME_RENDER_PRIMARY,
        session_id: 7,
        source_frame_id: 11,
        view_key: 13,
        native_frame: 17,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe {
        rsf_input_dump_configure(
            root_utf8.as_ptr(),
            c"contract\n\"test\"".as_ptr(),
            c"test".as_ptr(),
            Some(count),
            ptr::from_mut(&mut messages).cast(),
        );
        rsf_input_dump_request();
        assert_ne!(rsf_input_dump_begin_record(&frame, 1), 0);
        let stages = [
            (c"sr", 1, -3),
            (c"sequence_capture", 1, 0),
            (c"automatic_sr_failure", 1, -3),
            (c"spatial_recovery", 0, 0),
        ];
        for (stage, backend, result) in stages {
            rsf_input_dump_result(&frame, stage.as_ptr(), backend, result);
        }
        rsf_input_dump_result(&frame, c"../not_allowed".as_ptr(), 0, 0);
        let wrong_pass = rsf_game_render_pass {
            view_key: 14,
            ..pass
        };
        rsf_input_dump_pass_metadata(&wrong_pass);
        rsf_input_dump_pass_metadata(&pass);
        let valid = rsf_frame_record {
            resource_generation: 99,
            ..frame
        };
        let mut fg = rsf_fg_frame {
            struct_size: size_of::<rsf_fg_frame>() as u32,
            record: &valid,
            ..Default::default()
        };
        rsf_input_dump_fg_prepare(&pass, &fg, 0);
        let invalid = rsf_frame_record {
            view_id: 14,
            resource_generation: 100,
            ..frame
        };
        fg.record = &invalid;
        rsf_input_dump_fg_prepare(&pass, &fg, -1);
        rsf_input_dump_complete(&frame, 0);
        let bundle = bundles(&root).next().unwrap();
        for (stage, _, _) in stages {
            let file = format!("{}_status.json", stage.to_str().unwrap());
            assert!(contents(&bundle, &file).contains("\"source_frame_id\":11"));
        }
        assert!(contents(&bundle, "frame.json").contains("\"camera_available\":false"));
        let manifest = contents(&bundle, "manifest.json");
        assert!(manifest.contains("\"sr_result\":-3"));
        assert!(manifest.contains(r#""game":"contract\u000a\"test\"""#));
        assert!(
            !bundle
                .parent()
                .unwrap()
                .join("not_allowed_status.json")
                .exists()
        );
        let fg = contents(&bundle, "fg_constants.json");
        assert!(fg.contains("\"resource_generation\":99") && fg.contains("\"prepare_result\":0"));
        assert!(contents(&bundle, "engine_pass.json").contains("\"view_key\":13"));

        rsf_input_dump_request();
        frame.frame_id += 1;
        assert_ne!(rsf_input_dump_begin_record(&frame, 2), 0);
        let different = rsf_frame_record {
            resource_generation: 3,
            ..frame
        };
        let reserve = |frame: &rsf_frame_record, name: &std::ffi::CStr| {
            rsf_input_dump_reserve(
                frame,
                name.as_ptr(),
                name.as_ptr(),
                RSF_FIXTURE_DUMP_VIEW_RAW as u32,
                1.0,
            )
        };
        assert_eq!(rsf_input_dump_wants(&different), 0);
        assert_eq!(rsf_input_dump_begin_record(&different, 2), 0);
        assert_eq!(reserve(&different, c"other_generation"), 0);
        let ambiguous = rsf_frame_record {
            flags: RSF_FRAME_FLAG_AMBIGUOUS_ID | RSF_FRAME_FLAG_NO_FG,
            ..frame
        };
        assert_eq!(rsf_input_dump_wants(&ambiguous), 0);
        assert_eq!(rsf_input_dump_begin_record(&ambiguous, 2), 0);
        let ticket = reserve(&frame, c"final_color");
        assert_ne!(ticket, 0);
        assert_eq!(reserve(&frame, c"final_color"), 0);
        for name in [c"later", c"blocked_raw", c"blocked_metadata"] {
            assert_eq!(reserve(&frame, name), ticket);
        }
        rsf_input_dump_complete(&different, 0);
        assert_ne!(rsf_input_dump_requested(), 0);
        rsf_input_dump_complete(&frame, 0);
        assert_eq!(reserve(&frame, c"after_present"), 0);
        let later = bundles(&root).find(|path| *path != bundle).unwrap();
        assert!(contents(&later, "manifest.json").contains("\"status\":\"waiting_for_gpu\""));
        let source = [1u8, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8];
        let pixels = rsf_dump_pixels {
            struct_size: size_of::<rsf_dump_pixels>() as u32,
            width: 1,
            height: 2,
            format: 28,
            row_pitch: 8,
            data_size: source.len() as u64,
            data: source.as_ptr().cast(),
        };
        let write =
            |name: &std::ffi::CStr| rsf_input_dump_pixels(ticket, name.as_ptr(), &pixels, 3, 37);
        for (name, occupied, absent, preview) in [
            (
                c"blocked_raw",
                "blocked_raw.bin",
                "blocked_raw_raw.json",
                "blocked_raw.png",
            ),
            (
                c"blocked_metadata",
                "blocked_metadata_raw.json",
                "blocked_metadata.bin",
                "blocked_metadata.png",
            ),
        ] {
            fs::create_dir(later.join(occupied)).unwrap();
            assert_eq!(write(name), 0);
            assert!(!later.join(absent).exists());
            assert!(later.join(occupied).is_dir() && later.join(preview).is_file());
        }
        assert_ne!(write(c"final_color"), 0);
        assert_eq!(write(c"final_color"), 0);
        assert_ne!(rsf_input_dump_requested(), 0);
        rsf_input_dump_failed(
            ticket,
            c"later".as_ptr(),
            RSF_FIXTURE_TEXTURE_STAGING_FAILED,
        );
        assert_eq!(rsf_input_dump_requested(), 0);
        assert!(contents(&later, "manifest.json").contains("\"status\":\"partial\""));
        assert_eq!(
            fs::read(later.join("final_color.bin")).unwrap(),
            [1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert_eq!(
            contents(&later, "final_color_raw.json"),
            "{\"width\":1,\"height\":2,\"format\":28,\"native_api\":3,\"native_format\":37,\"row_pitch\":4,\"source_row_pitch\":8,\"bytes\":8}\n"
        );
        assert!(
            fs::read(later.join("final_color.png"))
                .unwrap()
                .starts_with(b"\x89PNG\r\n\x1a\n")
        );

        rsf_input_dump_request();
        frame.frame_id += 1;
        assert_ne!(rsf_input_dump_begin_record(&frame, 2), 0);
        let later_ticket = reserve(&frame, c"final_color");
        assert!(later_ticket != 0 && later_ticket != ticket);
        assert_eq!(write(c"final_color"), 0);
        rsf_input_dump_cancel(frame.session_id + 1);
        assert_ne!(rsf_input_dump_requested(), 0);
        rsf_input_dump_cancel(frame.session_id);
        assert_eq!(rsf_input_dump_requested(), 0);
        assert_ne!(messages, 0);

        rsf_input_dump_request();
        pass.role = RSF_GAME_RENDER_SR;
        rsf_input_dump_pass(ptr::null_mut(), &pass, 1);
        assert_ne!(rsf_input_dump_active(), 0);
        let window = rsf_game_render_pass {
            role: RSF_GAME_RENDER_WINDOW,
            scope_id: 45,
            window_key: 46,
            rhi_viewport_key: 47,
            ..pass
        };
        rsf_input_dump_present(ptr::null_mut(), ptr::null_mut(), &window);
        let other = rsf_game_render_pass {
            rhi_viewport_key: 48,
            ..window
        };
        rsf_input_dump_presented(&other, 0);
        assert_ne!(rsf_input_dump_requested(), 0);
        rsf_input_dump_presented(&window, 0);
        assert_eq!(rsf_input_dump_requested(), 0);
        assert_eq!(rsf_input_dump_active(), 0);
    }
    println!("PASS capture metadata, ticket retirement and CPU row encoding");
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_capture_metadata(path: *const c_char) -> i32 {
    entry(path, run)
}
