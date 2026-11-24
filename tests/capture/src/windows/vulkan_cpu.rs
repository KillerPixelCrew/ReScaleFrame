// SPDX-License-Identifier: MIT
use super::{bundles, contents, entry, root, utf8};
use crate::sys::*;
use ash::vk;
use rsf_game_sys::backend_result::{
    FEATURE_FAILED, INIT_FAILED, INVALID_ARGUMENT, NOT_READY, NOT_SUPPORTED,
};
use rsf_game_sys::{RSF_GAME_CAMERA_ABI_VERSION, rsf_frame_record};
use rsf_test_support::{frame_record, wide_os};
use std::cell::UnsafeCell;
use std::ffi::{CStr, c_char, c_void};
use std::{fs, path::Path, ptr};
use windows_sys::Win32::System::Environment::SetEnvironmentVariableW;

mod queue;
use queue::{Queue, device_proc, instance_proc, submit};

fn run(path: &Path, api: &rsf_fixture_vk_capture_api) -> i32 {
    let root = root(path, "vulkan-cpu");
    let legacy = root.join("legacy");
    let legacy_wide = wide_os(&legacy);
    unsafe {
        SetEnvironmentVariableW(
            windows::core::w!("RSF_VULKAN_FRAME_CAPTURE_DIR").as_ptr(),
            legacy_wide.as_ptr(),
        );
        SetEnvironmentVariableW(
            windows::core::w!("RSF_FRAME_CAPTURE_FIRST").as_ptr(),
            windows::core::w!("32").as_ptr(),
        );
        SetEnvironmentVariableW(
            windows::core::w!("RSF_FRAME_CAPTURE_COUNT").as_ptr(),
            windows::core::w!("1").as_ptr(),
        );
    }
    let (create, snapshot, drain, destroy) = (
        api.create.unwrap(),
        api.snapshot.unwrap(),
        api.drain.unwrap(),
        api.destroy.unwrap(),
    );
    let storage = Box::new(UnsafeCell::new(Queue::default()));
    let raw: *mut c_void = ptr::from_ref(storage.as_ref()).cast_mut().cast();
    // No queue reference may span a driver callback.
    let facts = || unsafe { &*storage.get() };
    let set = |update: fn(&mut Queue)| unsafe { update(&mut *storage.get()) };
    let setup = rsf_input_dump_vk_setup {
        struct_size: size_of::<rsf_input_dump_vk_setup>() as u32,
        abi_version: 1,
        graphics: rsf_srvk_setup {
            struct_size: size_of::<rsf_srvk_setup>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            instance: raw,
            physical_device: raw,
            device: raw,
            instance_proc: Some(instance_proc),
            device_proc: Some(device_proc),
            ..Default::default()
        },
        queue: raw,
        queue_family: 7,
        submit: Some(submit),
        user: raw,
    };
    let mut owner = ptr::null_mut();
    let frame = frame_record(31, 32, 33, [1, 1], [1, 1]);
    let image = rsf_srvk_image {
        device: raw,
        image: 1,
        format: vk::Format::R8G8B8A8_UNORM.as_raw() as u32,
        width: 1,
        height: 1,
        layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL.as_raw() as u32,
        copy_source: 1,
    };
    unsafe {
        assert_eq!(create(&setup, ptr::null_mut()), INVALID_ARGUMENT);
        let short = rsf_input_dump_vk_setup {
            struct_size: 4,
            ..setup
        };
        assert_eq!(create(&short, &mut owner), INVALID_ARGUMENT);
        assert!(owner.is_null());
        set(|q| q.missing_wait = true);
        assert_eq!(create(&setup, &mut owner), NOT_SUPPORTED);
        assert!(owner.is_null());
        assert_eq!(facts().pools, 0);
        set(|q| {
            q.missing_wait = false;
            q.fail_fence = true;
        });
        assert_eq!(create(&setup, &mut owner), INIT_FAILED);
        assert!(owner.is_null());
        assert_eq!(
            (facts().pools, facts().pool_destroys, facts().fence_destroys),
            (1, 1, 0)
        );
        set(|q| q.fail_fence = false);
        assert_eq!(create(&setup, &mut owner), 0);
        assert!(!owner.is_null());
        let snap = |name: &CStr| snapshot(owner, &frame, &image, name.as_ptr());
        assert_eq!(snap(c"unrequested"), 0);
        assert_eq!(
            (
                facts().resets,
                facts().submits,
                facts().maps,
                facts().waits,
                facts().buffers
            ),
            (0, 0, 0, 0, 0)
        );
        rsf_input_dump_configure(
            utf8(&root).as_ptr(),
            c"Vulkan CPU fixture".as_ptr(),
            c"fixture".as_ptr(),
            None,
            ptr::null_mut(),
        );
        rsf_input_dump_request();
        assert_ne!(rsf_input_dump_begin_record(&frame, 0), 0);
        let bundle = bundles(&root).next().unwrap();
        let initial = contents(&bundle, "frame.json");
        let mut evaluated = frame;
        evaluated.camera.struct_size = size_of_val(&evaluated.camera) as u32;
        evaluated.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
        evaluated.camera.render_width = 1;
        evaluated.camera.render_height = 1;
        evaluated.camera.output_width = 1;
        evaluated.camera.output_height = 1;
        evaluated.camera.jitter_pixels[0] = 0.25;
        rsf_input_dump_result(&evaluated, c"sr".as_ptr(), 2, -1);
        for stale in [
            rsf_frame_record {
                resource_generation: 1,
                ..evaluated
            },
            rsf_frame_record {
                output_width: 2,
                ..evaluated
            },
        ] {
            rsf_input_dump_result(&stale, c"sr".as_ptr(), 2, 0);
        }
        assert_eq!(contents(&bundle, "frame.json"), initial);
        rsf_input_dump_result(&evaluated, c"sr".as_ptr(), 2, 0);
        let accepted = contents(&bundle, "frame.json");
        for fragment in [
            "\"camera_available\":true",
            "\"backend\":2",
            "\"jitter_pixels\":[0.25,0]",
        ] {
            assert!(accepted.contains(fragment));
        }
        evaluated.camera.jitter_pixels[0] = -0.25;
        rsf_input_dump_result(&evaluated, c"sr".as_ptr(), 3, 0);
        assert_eq!(contents(&bundle, "frame.json"), accepted);
        let unsupported = rsf_srvk_image {
            copy_source: 0,
            ..image
        };
        assert_eq!(
            snapshot(owner, &frame, &unsupported, c"unsupported".as_ptr()),
            INVALID_ARGUMENT
        );
        assert_eq!(
            (facts().resets, facts().buffers, facts().submits),
            (0, 0, 0)
        );
        let reserve = |name: &CStr| {
            rsf_input_dump_reserve(
                &frame,
                name.as_ptr(),
                name.as_ptr(),
                RSF_FIXTURE_DUMP_VIEW_HDR as u32,
                1.0,
            )
        };
        let engine = reserve(c"engine");
        assert_ne!(engine, 0);
        set(|q| q.fail_end = true);
        assert_eq!(snap(c"refused_end"), FEATURE_FAILED);
        assert_eq!(
            (
                facts().submits,
                facts().copies_recorded,
                facts().frees,
                facts().last().live
            ),
            (0, 1, 1, false)
        );
        set(|q| q.fail_end = false);
        assert_eq!(snap(c"successful"), 0);
        assert_eq!(
            (
                facts().submits,
                facts().maps,
                facts().invalid_maps,
                facts().frees,
                facts().timeout
            ),
            (1, 1, 0, 2, 15_000_000_000)
        );
        set(|q| q.submit_result = vk::Result::ERROR_OUT_OF_DEVICE_MEMORY);
        let waits = facts().waits;
        assert_eq!(snap(c"refused_submit"), FEATURE_FAILED);
        assert_eq!(
            (facts().submits, facts().waits, facts().maps, facts().frees),
            (2, waits, 1, 3)
        );
        set(|q| q.submit_result = vk::Result::SUCCESS);
        assert_eq!(drain(owner), 0);
        assert_eq!((facts().markers, facts().maps), (1, 1));
        set(|q| q.fail_bind = true);
        assert_eq!(snap(c"refused_bind"), FEATURE_FAILED);
        assert_eq!(
            (
                facts().submits,
                facts().copies_recorded,
                facts().frees,
                facts().last().live
            ),
            (2, 3, 4, false)
        );
        set(|q| q.fail_bind = false);
        let later = reserve(c"later_engine");
        assert_ne!(later, 0);
        set(|q| q.stalled = true);
        assert_eq!(snap(c"final_color"), NOT_READY);
        assert!(facts().pending && facts().last().live && facts().last().memory);
        assert_eq!((facts().maps, facts().frees), (1, 4));
        let waits = facts().waits;
        let absent = rsf_frame_record {
            frame_id: 33,
            ..frame
        };
        assert_eq!(snapshot(owner, &absent, &image, c"absent".as_ptr()), 0);
        assert_eq!(facts().waits, waits);
        let resets = facts().resets;
        assert_eq!(snap(c"blocked"), NOT_READY);
        assert_eq!(
            (facts().resets, facts().timeout, facts().submits),
            (resets, 0, 3)
        );
        assert_eq!(destroy(owner), NOT_READY);
        assert_eq!((facts().fence_destroys, facts().pool_destroys), (0, 1));
        assert_eq!(drain(owner), NOT_READY);
        assert_eq!((facts().markers, facts().maps, facts().frees), (1, 1, 4));
        assert_eq!(
            rsf_input_dump_gpu_ready(later, c"later_engine".as_ptr(), 3, raw),
            0
        );
        set(|q| q.stalled = false);
        assert_eq!(drain(owner), 0);
        assert_eq!(
            (
                facts().markers,
                facts().maps,
                facts().invalid_maps,
                facts().frees,
                facts().pending
            ),
            (2, 2, 0, 5, false)
        );
        assert_eq!(
            rsf_input_dump_gpu_ready(later, c"later_engine".as_ptr(), 3, raw),
            0
        );
        rsf_input_dump_failed(
            engine,
            c"engine".as_ptr(),
            RSF_FIXTURE_TEXTURE_STAGING_FAILED,
        );
        rsf_input_dump_failed(
            later,
            c"later_engine".as_ptr(),
            RSF_FIXTURE_TEXTURE_STAGING_FAILED,
        );
        rsf_input_dump_complete(&frame, 0);
        for role in ["successful", "final_color"] {
            assert_eq!(
                fs::read(bundle.join(format!("{role}.bin"))).unwrap(),
                [64, 128, 255, 0]
            );
            assert_eq!(
                fs::read(legacy.join(format!("31-32-v33-g0-{role}-0.raw"))).unwrap(),
                [64, 128, 255, 0]
            );
        }
        for role in ["refused_bind", "refused_end", "refused_submit", "blocked"] {
            assert!(!bundle.join(format!("{role}.bin")).exists());
            assert!(!legacy.join(format!("31-32-v33-g0-{role}-0.raw")).exists());
        }
        set(|q| q.stalled = true);
        assert_eq!(drain(owner), NOT_READY);
        assert_eq!((facts().markers, facts().pending), (3, true));
        assert_eq!(destroy(owner), NOT_READY);
        assert_eq!(facts().fence_destroys, 0);
        set(|q| q.stalled = false);
        assert_eq!(destroy(owner), 0);
        assert_eq!(
            (
                facts().pool_destroys,
                facts().fence_destroys,
                facts().markers,
                facts().maps
            ),
            (2, 1, 3, 2)
        );
        assert_eq!(facts().buffers, facts().frees);
        assert_eq!(drain(ptr::null_mut()), 0);
        assert_eq!(destroy(ptr::null_mut()), 0);

        assert_eq!(create(&setup, &mut owner), 0);
        rsf_input_dump_request();
        assert_ne!(rsf_input_dump_begin_record(&frame, 2), 0);
        set(|q| {
            q.stalled = true;
            q.lost_on_wait = true;
        });
        assert_eq!(
            snapshot(owner, &frame, &image, c"lost_device".as_ptr()),
            RSF_INPUT_DUMP_VK_DEVICE_LOST
        );
        assert_eq!(
            (facts().pending, facts().last().live, facts().frees),
            (false, true, 5)
        );
        assert_eq!(drain(owner), RSF_INPUT_DUMP_VK_DEVICE_LOST);
        assert_eq!(destroy(owner), 0);
        assert_eq!((facts().maps, facts().frees), (2, 6));
        assert!(!legacy.join("31-32-v33-g0-lost_device-0.raw").exists());
        rsf_input_dump_cancel(0);

        assert_eq!(create(&setup, &mut owner), 0);
        rsf_input_dump_request();
        assert_ne!(rsf_input_dump_begin_record(&frame, 2), 0);
        set(|q| {
            q.lost_on_wait = false;
            q.submit_result = vk::Result::ERROR_DEVICE_LOST;
        });
        assert_eq!(
            snapshot(owner, &frame, &image, c"uncertain_submit".as_ptr()),
            RSF_INPUT_DUMP_VK_DEVICE_LOST
        );
        assert_eq!(
            (facts().pending, facts().last().live, facts().frees),
            (true, true, 6)
        );
        assert_eq!(destroy(owner), NOT_READY);
        set(|q| q.lost_on_wait = true);
        assert_eq!(destroy(owner), 0);
        assert_eq!((facts().maps, facts().frees), (2, 7));
        assert!(!legacy.join("31-32-v33-g0-uncertain_submit-0.raw").exists());
        rsf_input_dump_cancel(0);
    }
    assert!(facts().valid(), "Vulkan call contract");
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_input_dump_vulkan_cpu(
    path: *const c_char,
    api: &rsf_fixture_vk_capture_api,
) -> i32 {
    entry(path, |path| run(path, api))
}
