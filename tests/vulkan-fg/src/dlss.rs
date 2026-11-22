// SPDX-License-Identifier: MIT
mod sdk;
mod vulkan;

use crate::sys::*;
use ash::vk::{self, Handle as _};
use rsf_game_sys::{RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record};
use std::{
    ffi::{CStr, c_void},
    ptr,
};

type Owner = *mut rsf_dlssgvk;
type Handle = *mut c_void;
const OK: i32 = FIXTURE_OK;
const COLOR: vk::ImageUsageFlags = vk::ImageUsageFlags::COLOR_ATTACHMENT;
const SRC: vk::ImageUsageFlags = vk::ImageUsageFlags::TRANSFER_SRC;
const DST: vk::ImageUsageFlags = vk::ImageUsageFlags::TRANSFER_DST;
const OUTPUT: vk::Extent2D = vk::Extent2D {
    width: 1280,
    height: 720,
};
const RENDER: vk::Extent2D = vk::Extent2D {
    width: 640,
    height: 360,
};
const FORMAT: vk::Format = vk::Format::B8G8R8A8_UNORM;

fn handle(id: usize) -> Handle {
    ptr::without_provenance_mut(id)
}

fn source(session: u64, id: u64) -> rsf_frame_record {
    let mut record = rsf_frame_record {
        struct_size: size_of::<rsf_frame_record>() as u32,
        abi_version: RSF_GAME_FRAME_ABI_VERSION,
        session_id: session,
        frame_id: id,
        render_width: RENDER.width,
        render_height: RENDER.height,
        output_width: OUTPUT.width,
        output_height: OUTPUT.height,
        frame_time_ms: 16.0,
        ..unsafe { std::mem::zeroed() }
    };
    let camera = &mut record.camera;
    camera.struct_size = size_of_val(camera) as u32;
    camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    camera.render_width = RENDER.width;
    camera.render_height = RENDER.height;
    camera.output_width = OUTPUT.width;
    camera.output_height = OUTPUT.height;
    camera.near_plane = 0.1;
    camera.far_plane = 500.0;
    camera.vertical_fov_radians = 1.0;
    for index in (0..16).step_by(5) {
        camera.clip_to_previous_clip[index] = 1.0;
    }
    record
}

fn input(record: &rsf_frame_record) -> rsf_fgvk_frame {
    let image = |image, format: vk::Format, extent: vk::Extent2D| rsf_srvk_image {
        device: handle(1),
        image,
        format: format.as_raw() as u32,
        width: extent.width,
        height: extent.height,
        layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL.as_raw() as u32,
        copy_source: 1,
    };
    rsf_fgvk_frame {
        struct_size: size_of::<rsf_fgvk_frame>() as u32,
        abi_version: RSF_FGVK_ABI_VERSION,
        record,
        depth: image(99, vk::Format::R32_SFLOAT, RENDER),
        motion: image(98, vk::Format::R16G16_SFLOAT, RENDER),
        hudless: image(97, vk::Format::R8G8B8A8_UNORM, OUTPUT),
        motion_scale_x: 320.0,
        motion_scale_y: -180.0,
    }
}

fn status(owner: Owner) -> rsf_dlssgvk_status {
    let mut result = rsf_dlssgvk_status {
        struct_size: size_of::<rsf_dlssgvk_status>() as u32,
        abi_version: RSF_DLSSGVK_ABI_VERSION,
        ..Default::default()
    };
    assert_eq!(unsafe { rsf_dlssgvk_get_status(owner, &mut result) }, OK);
    result
}

fn capture_state() -> rsf_dlss_fixture_capture_state {
    let mut result = rsf_dlss_fixture_capture_state::default();
    unsafe { rsf_fixture_dlss_capture_state(&mut result) };
    result
}

fn verify_present_capture(setup: &rsf_fgvk_setup, record: &rsf_frame_record) {
    let mut capture = ptr::null_mut();
    let mut leaves = rsf_dlss_fixture_capture_calls::default();
    let chain_info = vk::SwapchainCreateInfoKHR::default()
        .image_usage(SRC | DST)
        .image_format(FORMAT)
        .image_array_layers(1)
        .image_extent(OUTPUT);
    let chains = [vk::SwapchainKHR::from_raw(0x33)];
    let indices = [1];
    let waits = [
        vk::Semaphore::from_raw(0x601),
        vk::Semaphore::from_raw(0x602),
    ];
    let extension = 0i32;
    let info = vk::PresentInfoKHR {
        p_next: ptr::from_ref(&extension).cast(),
        ..vk::PresentInfoKHR::default()
            .wait_semaphores(&waits)
            .swapchains(&chains)
            .image_indices(&indices)
    };
    const SUBOPTIMAL: i32 = vk::Result::SUBOPTIMAL_KHR.as_raw();
    unsafe {
        assert_eq!(
            rsf_fixture_dlss_capture_open(setup, &mut capture, &mut leaves),
            OK
        );
        let admitted = capture_state();
        assert!(!capture.is_null() && admitted.user == capture.cast());
        assert_eq!((admitted.registered, admitted.registrations), (1, 1));
        let sdk = rsf_dlssgvk_status {
            struct_size: size_of::<rsf_dlssgvk_status>() as u32,
            abi_version: RSF_DLSSGVK_ABI_VERSION,
            ..Default::default()
        };
        rsf_core_dlss_capture_remember(capture, record, 3, &sdk, 1);
        let mut packet = rsf_sl_present_capture_packet {
            struct_size: size_of::<rsf_sl_present_capture_packet>() as u32,
            abi_version: RSF_SL_PRESENT_CAPTURE_ABI_VERSION,
            device: handle(1),
            queue: handle(2),
            present_info: ptr::from_ref(&info).cast(),
            swapchain_info: ptr::from_ref(&chain_info).cast(),
            native_present: leaves.present,
            source_frame: 42,
            present_frame: 43,
            generated_index: 2,
            boundary_valid: 1,
            queue_family: 5,
            native_images: leaves.images,
            native_submit: leaves.submit,
            native_wsi_call: leaves.wsi,
        };
        assert_eq!(rsf_fixture_dlss_capture_call(&packet), SUBOPTIMAL);
        let observed = capture_state();
        assert_eq!(
            (
                observed.pool_family,
                observed.pool_flags,
                observed.begins,
                observed.ends
            ),
            (5, vk::CommandPoolCreateFlags::TRANSIENT.as_raw(), 1, 1)
        );
        let all_commands = vk::PipelineStageFlags::ALL_COMMANDS.as_raw();
        assert_eq!(
            (
                observed.real_submits,
                observed.wait_count,
                observed.waits,
                observed.stages
            ),
            (1, 2, waits.as_ptr().addr(), [all_commands; 2])
        );
        assert_eq!((observed.command_count, observed.commands), (1, 0x501));
        assert_ne!(observed.submitted_fence, 0);
        assert_eq!(
            (
                observed.waited_fence,
                observed.timeout,
                observed.flushes,
                observed.metadata
            ),
            (observed.submitted_fence, 15_000_000_000, 1, 1)
        );
        assert_eq!(
            (
                observed.real_enumerations,
                observed.real_presents,
                observed.wsi_calls
            ),
            (2, 1, 3)
        );
        assert_ne!(observed.present_info, ptr::from_ref(&info).addr());
        assert_eq!(
            (observed.present_wait_count, observed.present_waits),
            (0, 0)
        );
        assert_eq!(observed.present_extension, ptr::from_ref(&extension).addr());
        assert_eq!(
            (
                observed.present_chain_count,
                observed.present_chains,
                observed.present_indices
            ),
            (1, chains.as_ptr().addr(), indices.as_ptr().addr())
        );

        packet.source_frame = 7;
        assert_eq!(rsf_fixture_dlss_capture_call(&packet), SUBOPTIMAL);
        let unmapped = capture_state();
        assert_eq!(
            (
                unmapped.present_info,
                unmapped.real_submits,
                unmapped.real_presents
            ),
            (ptr::from_ref(&info).addr(), 1, 2)
        );
        packet.source_frame = 42;
        let no_source = chain_info.image_usage(DST);
        packet.swapchain_info = ptr::from_ref(&no_source).cast();
        assert_eq!(rsf_fixture_dlss_capture_call(&packet), SUBOPTIMAL);
        let unchanged = capture_state();
        assert_eq!(
            (
                unchanged.present_info,
                unchanged.real_enumerations,
                unchanged.real_submits
            ),
            (ptr::from_ref(&info).addr(), 2, 1)
        );
        assert!(!unchanged.failure.is_null());
        assert_eq!(
            CStr::from_ptr(unchanged.failure),
            c"real-chain-copy-contract"
        );
        assert_eq!(
            unchanged.failure_result,
            vk::Result::ERROR_FORMAT_NOT_SUPPORTED.as_raw()
        );
        packet.native_wsi_call = 0;
        assert_eq!(
            rsf_fixture_dlss_capture_call(&packet),
            vk::Result::ERROR_INITIALIZATION_FAILED.as_raw()
        );
        assert_eq!(capture_state().real_presents, 3);

        assert_eq!(rsf_core_dlss_capture_detach(capture), OK);
        let detached = capture_state();
        assert_eq!((detached.registered, detached.registrations), (0, 2));
        rsf_core_dlss_capture_close(capture);
        let closed = capture_state();
        assert_eq!(
            (closed.destroyed_pools, closed.destroyed_fence),
            (1, observed.submitted_fence)
        );
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_dlss_run() -> i32 {
    // Capture stays disabled for the provider scenarios.
    unsafe { std::env::remove_var("RSF_VULKAN_FRAME_CAPTURE_DIR") };
    // Read or mutate the native doubles only between calls into the provider.
    let state = unsafe { rsf_fixture_dlss_state() };
    macro_rules! f {
        ($field:ident) => {
            (*state).$field
        };
    }
    let device = handle(1);
    let queue = handle(2);
    let chain = vk::SwapchainKHR::from_raw(3);
    let create = vk::SwapchainCreateInfoKHR::default()
        .surface(vk::SurfaceKHR::from_raw(5))
        .image_extent(OUTPUT)
        .image_format(FORMAT)
        .image_array_layers(1)
        .image_usage(COLOR);
    let mut setup = rsf_fgvk_setup {
        struct_size: size_of::<rsf_fgvk_setup>() as u32,
        abi_version: RSF_FGVK_ABI_VERSION,
        graphics: rsf_srvk_setup {
            struct_size: size_of::<rsf_srvk_setup>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            instance: handle(6),
            physical_device: handle(7),
            device,
            instance_proc: Some(rsf_fixture_dlss_proc),
            device_proc: Some(rsf_fixture_dlss_proc),
            output_width: OUTPUT.width,
            output_height: OUTPUT.height,
            ..Default::default()
        },
        queues: [queue, ptr::null_mut(), ptr::null_mut(), ptr::null_mut()],
        queue_user: queue,
        allocator: handle(4),
        graphics_submit: Some(rsf_fixture_dlss_submit),
        lock_queues: Some(rsf_fixture_dlss_lock),
        unlock_queues: Some(rsf_fixture_dlss_unlock),
        ..Default::default()
    };
    let mut owner = ptr::null_mut();
    let mut chain_out = 0;
    let commands = [handle(10), handle(11)];
    let owned = [chain];
    let foreign = [vk::SwapchainKHR::from_raw(99)];
    let info = vk::PresentInfoKHR::default().swapchains(&owned);
    let foreign_info = vk::PresentInfoKHR::default().swapchains(&foreign);
    let present = |owner| unsafe { rsf_dlssgvk_present(owner, ptr::from_ref(&info).cast()) };
    let explicit = |owner, record| unsafe {
        rsf_dlssgvk_present_source(owner, ptr::from_ref(&info).cast(), record)
    };
    let stage = |owner: Owner, record: &rsf_frame_record| {
        let input = input(record);
        assert_eq!(
            unsafe { rsf_dlssgvk_prepare(owner, commands[0], &input) },
            OK
        );
        assert_eq!(
            unsafe { rsf_dlssgvk_hudless(owner, commands[1], &input) },
            OK
        );
    };
    let submitted =
        |owner, count| unsafe { rsf_dlssgvk_submitted(owner, commands.as_ptr(), count) };
    unsafe {
        assert_eq!(
            rsf_dlssgvk_create(ptr::null(), ptr::null_mut(), ptr::null_mut()),
            FIXTURE_INVALID_ARGUMENT
        );
        assert_eq!(
            rsf_dlssgvk_prepare(ptr::null_mut(), ptr::null_mut(), ptr::null()),
            FIXTURE_INVALID_ARGUMENT
        );
        assert_eq!(rsf_dlssgvk_destroy(ptr::null_mut()), OK);

        let bad_type = vk::StructureType::DEVICE_CREATE_INFO;
        setup.create_info = ptr::from_ref(&bad_type).cast();
        assert_eq!(
            rsf_fixture_dlss_create(RSF_DLSS_FIXTURE_NORMAL, &setup, &mut owner, &mut chain_out),
            FIXTURE_INVALID_ARGUMENT
        );
        assert!(
            owner.is_null() && chain_out == 0 && f!(chain_creates) == 0,
            "foreign Vulkan struct prefix"
        );
        let layered = create.image_array_layers(2);
        setup.create_info = ptr::from_ref(&layered).cast();
        assert_eq!(
            rsf_fixture_dlss_create(RSF_DLSS_FIXTURE_NORMAL, &setup, &mut owner, &mut chain_out),
            FIXTURE_INVALID_ARGUMENT
        );
        assert!(
            owner.is_null() && chain_out == 0 && f!(chain_creates) == 0,
            "unsupported layer count"
        );
        setup.create_info = ptr::from_ref(&create).cast();
        assert_eq!(
            rsf_fixture_dlss_create(RSF_DLSS_FIXTURE_NORMAL, &setup, &mut owner, &mut chain_out),
            OK
        );
        assert!(!owner.is_null() && chain_out == chain.as_raw());

        let (mut second, mut second_chain) = (ptr::null_mut(), 0);
        assert_eq!(
            rsf_fixture_dlss_create(
                RSF_DLSS_FIXTURE_NORMAL,
                &setup,
                &mut second,
                &mut second_chain
            ),
            FIXTURE_NOT_READY
        );
        assert!(
            second.is_null() && second_chain == 0,
            "singleton viewport claim"
        );
        let mut images = [0; 8];
        let mut count = 8;
        assert_eq!(
            rsf_dlssgvk_images(owner, &mut count, images.as_mut_ptr()),
            vk::Result::SUCCESS.as_raw()
        );
        assert_eq!((count, images[2]), (3, 102));
        count = 2;
        assert_eq!(
            rsf_dlssgvk_images(owner, &mut count, images.as_mut_ptr()),
            vk::Result::INCOMPLETE.as_raw()
        );
        assert_eq!(count, 2);
        let (mut index, mut layout) = (0, 0);
        assert_eq!(
            rsf_dlssgvk_acquire(owner, 8, 9, 10, &mut index),
            vk::Result::SUBOPTIMAL_KHR.as_raw()
        );
        assert_eq!(index, 2);
        assert_eq!(rsf_dlssgvk_image_layout(owner, &mut layout), OK);
        assert_eq!(
            layout,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL.as_raw() as u32
        );
        assert_eq!(
            rsf_dlssgvk_image_layout(owner, ptr::null_mut()),
            FIXTURE_INVALID_ARGUMENT
        );
        assert_eq!(rsf_dlssgvk_enable(owner, 1, 4), FIXTURE_NOT_SUPPORTED);
        assert_eq!(rsf_dlssgvk_enable(owner, 1, 0), FIXTURE_INVALID_ARGUMENT);
        assert_eq!(rsf_dlssgvk_enable(owner, 1, 3), OK);

        f!(lease_frame) = 42;
        stage(owner, &source(f!(lease_session), 42));
        assert_eq!(present(owner), OK);
        assert_eq!(
            (
                f!(on_calls),
                f!(off_calls),
                status(owner).presentation.enabled
            ),
            (0, 1, 0)
        );
        f!(lease_frame) = 43;
        stage(owner, &source(f!(lease_session), 43));
        submitted(owner, 1);
        assert_eq!(f!(fence_submits), 1);
        assert_eq!(present(owner), OK);
        assert_eq!(f!(off_calls), 2, "prepare-only source remains Off");

        f!(lease_frame) = 44;
        stage(owner, &source(f!(lease_session), 44));
        submitted(owner, 2);
        let previous = f!(fence_submits);
        submitted(owner, 2);
        assert_eq!(
            f!(fence_submits),
            previous,
            "duplicate submission keeps the live fence"
        );
        f!(sdk_presented) = 4;
        assert_eq!(present(owner), OK);
        assert_eq!(f!(on_calls), 1);
        let mut snapshot = status(owner);
        assert_eq!(
            (
                snapshot.presentation.enabled,
                snapshot.presentation.source_frame,
                snapshot.presentation.last_present_count,
                snapshot.presentation.generated_callbacks
            ),
            (1, 44, 4, 0)
        );
        assert_eq!(present(owner), OK);
        assert_eq!(status(owner).presentation.enabled, 0);

        f!(lease_frame) = 50;
        let recycle = source(f!(lease_session), 50);
        f!(refuse_inputs_wait) = true;
        let previous = f!(copies);
        assert_eq!(
            rsf_dlssgvk_prepare(owner, commands[0], &input(&recycle)),
            FIXTURE_NOT_READY
        );
        assert_eq!(f!(copies), previous, "timeline timeout retains the slot");
        f!(refuse_inputs_wait) = false;
        stage(owner, &recycle);
        assert_eq!(f!(inputs_waits), 2);
        submitted(owner, 2);
        f!(completion_value) = 0;
        assert_eq!(present(owner), OK);
        assert_eq!(f!(on_calls), 2);
        f!(expected_inputs_value) = 0;
        f!(lease_frame) = 56;
        stage(owner, &source(f!(lease_session), 56));
        assert_eq!(f!(inputs_waits), 3, "zero is a valid SDK timeline value");
        submitted(owner, 2);
        f!(completion_value) = 9;
        f!(expected_inputs_value) = 9;
        f!(lease_frame) = 57;
        assert_eq!(present(owner), OK);
        assert_eq!(
            (f!(on_calls), status(owner).presentation.enabled),
            (2, 0),
            "expired token"
        );

        stage(owner, &source(f!(lease_session), 57));
        f!(refuse_submit) = true;
        submitted(owner, 2);
        f!(refuse_submit) = false;
        assert_eq!(present(owner), OK);
        assert_eq!(f!(on_calls), 2, "unknown fence completion");
        f!(lease_session) = 8;
        f!(lease_frame) = 1;
        let old_output = source(8, 1);
        stage(owner, &old_output);
        submitted(owner, 2);
        f!(lease_frame) = 2;
        let new_output = source(8, 2);
        stage(owner, &new_output);
        submitted(owner, 2);
        f!(lease_frame) = 1;
        assert_eq!(explicit(owner, &old_output), OK);
        assert_eq!(
            (
                status(owner).presentation.enabled,
                status(owner).presentation.source_frame
            ),
            (1, 1)
        );
        f!(lease_frame) = 2;
        assert_eq!(explicit(owner, ptr::null()), OK);
        assert_eq!(status(owner).presentation.source_frame, 0);
        assert_eq!(explicit(owner, &new_output), OK);
        assert_eq!(
            (
                status(owner).presentation.enabled,
                status(owner).presentation.source_frame
            ),
            (1, 2)
        );
        for record in [&old_output, &new_output] {
            assert_eq!(explicit(owner, record), OK);
            assert_eq!(
                status(owner).presentation.enabled,
                0,
                "consumed identity does not rewind"
            );
        }

        f!(lease_frame) = 4;
        stage(owner, &source(8, 4));
        submitted(owner, 2);
        f!(refuse_options) = true;
        let previous = f!(present_calls);
        assert_eq!(
            present(owner),
            vk::Result::ERROR_INITIALIZATION_FAILED.as_raw()
        );
        assert_eq!(
            (f!(present_calls), status(owner).presentation.enabled),
            (previous, 0)
        );
        f!(refuse_options) = false;
        assert_eq!(
            rsf_dlssgvk_present(owner, ptr::from_ref(&foreign_info).cast()),
            vk::Result::ERROR_INITIALIZATION_FAILED.as_raw()
        );
        assert_eq!(
            f!(present_calls),
            previous,
            "foreign chain stays outside WSI"
        );
        snapshot.abi_version = 99;
        assert_eq!(
            rsf_dlssgvk_get_status(owner, &mut snapshot),
            FIXTURE_ABI_MISMATCH
        );
        assert_eq!(status(owner).requested_generated_frames, 3);

        f!(worker_timeout) = true;
        assert_eq!(rsf_dlssgvk_destroy(owner), FIXTURE_NOT_READY);
        assert_eq!((f!(destroyed_chains), f!(release_calls)), (0, 0));
        f!(worker_timeout) = false;
        f!(refuse_inputs_wait) = true;
        assert_eq!(rsf_dlssgvk_destroy(owner), FIXTURE_NOT_READY);
        assert_eq!((f!(destroyed_chains), f!(release_calls)), (0, 0));
        f!(refuse_inputs_wait) = false;
        assert_eq!(rsf_dlssgvk_drain(owner), OK);
        assert_eq!(status(owner).requested_generated_frames, 3);
        f!(lease_frame) = 5;
        f!(refuse_tags) = true;
        assert_eq!(
            rsf_dlssgvk_prepare(owner, commands[0], &input(&source(8, 5))),
            FIXTURE_FEATURE_FAILED
        );
        f!(refuse_tags) = false;
        let previous = f!(off_calls);
        assert_eq!(rsf_dlssgvk_destroy(owner), FIXTURE_NOT_READY);
        assert_eq!((f!(destroyed_chains), f!(off_calls)), (0, previous));
        let previous = f!(fence_submits);
        submitted(owner, 1);
        assert_eq!(f!(fence_submits), previous + 1);
        assert_eq!(rsf_dlssgvk_drain(owner), OK);
        f!(refuse_release) = true;
        assert_eq!(rsf_dlssgvk_destroy(owner), FIXTURE_FEATURE_FAILED);
        assert_eq!((f!(release_calls), f!(destroyed_chains)), (1, 0));
        f!(refuse_release) = false;
        assert_eq!(rsf_dlssgvk_destroy(owner), OK);
        assert_eq!((f!(release_calls), f!(destroyed_chains)), (2, 1));

        assert_eq!(
            rsf_fixture_dlss_create(RSF_DLSS_FIXTURE_NORMAL, &setup, &mut owner, &mut chain_out),
            OK
        );
        assert_eq!(present(owner), OK);
        assert_eq!(status(owner).presentation.enabled, 0);
        assert_eq!(rsf_dlssgvk_destroy(owner), OK);
        assert_eq!(f!(destroyed_chains), 2);
        let mut refused = |variant, expected, destroyed| {
            assert_eq!(
                rsf_fixture_dlss_create(variant, &setup, &mut owner, &mut chain_out),
                expected
            );
            assert!(owner.is_null() && chain_out == 0);
            // A fresh pointer, so the closure holds no borrow of the state.
            assert_eq!((*rsf_fixture_dlss_state()).destroyed_chains, destroyed);
        };
        f!(max_generated) = 0;
        refused(RSF_DLSS_FIXTURE_NORMAL, FIXTURE_NOT_SUPPORTED, 2);
        f!(max_generated) = 3;
        f!(refuse_create) = true;
        refused(RSF_DLSS_FIXTURE_NORMAL, FIXTURE_INIT_FAILED, 2);
        f!(refuse_create) = false;
        f!(refuse_enumeration) = true;
        refused(RSF_DLSS_FIXTURE_NORMAL, FIXTURE_INIT_FAILED, 3);
        f!(refuse_enumeration) = false;
        f!(backbuffer_count) = 0;
        refused(RSF_DLSS_FIXTURE_NORMAL, FIXTURE_INIT_FAILED, 4);
        f!(backbuffer_count) = 3;
        let previous = f!(chain_creates);
        refused(RSF_DLSS_FIXTURE_CAPTURE_REQUIRED, FIXTURE_NOT_SUPPORTED, 4);
        assert_eq!(f!(chain_creates), previous);
        f!(surface_usage) = (COLOR | SRC).as_raw();
        refused(RSF_DLSS_FIXTURE_NORMAL, FIXTURE_NOT_SUPPORTED, 4);
        assert_eq!(f!(chain_creates), previous);
        f!(surface_usage) |= DST.as_raw();
        f!(expected_chain_usage) = f!(surface_usage);
        refused(RSF_DLSS_FIXTURE_CAPTURE_REFUSED, FIXTURE_FEATURE_FAILED, 5);

        assert_eq!(
            rsf_fixture_dlss_create(RSF_DLSS_FIXTURE_NORMAL, &setup, &mut owner, &mut chain_out),
            OK
        );
        assert_ne!(chain_out, 0);
        assert_eq!(create.image_usage.as_raw(), COLOR.as_raw());
        assert_eq!(rsf_dlssgvk_destroy(owner), OK);
        assert_eq!(f!(destroyed_chains), 6);
        f!(lease_frame) = 42;
        verify_present_capture(&setup, &source(f!(lease_session), 42));
    }
    println!("Synthetic DLSS Vulkan FG identity, MFG and WSI checks passed");
    0
}
