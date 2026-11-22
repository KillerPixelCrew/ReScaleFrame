// SPDX-License-Identifier: MIT
use crate::vulkan_fg_abi::{RSF_FGVK_ABI_VERSION, rsf_fgvk_frame, rsf_fgvk_status};
use crate::vulkan_sr_abi::rsf_srvk_setup;
use ash::vk::{self, Handle as _};
use rsf_game_sys::{
    RSF_FRAME_FLAG_RESET, RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record,
};
use std::{ffi::c_void, mem, ptr::null_mut};

type Handle = *mut c_void;

unsafe extern "C" {
    fn rsf_fixture_xess_present_setup(
        graphics: *const rsf_srvk_setup,
        family: u32,
        runtime: *const u16,
        native: *mut Handle,
        owner: *mut Handle,
        chain: *mut u64,
    ) -> i32;
    fn rsf_fixture_xess_present_resources(native: Handle, owner: Handle, family: u32) -> i32;
    fn rsf_fixture_xess_present_idle(native: Handle, pump_messages: bool) -> i32;
    fn rsf_fixture_xess_present_record(
        native: Handle,
        frame: u32,
        index: u32,
        initialized: bool,
        command: *mut Handle,
        inputs: *mut rsf_fgvk_frame,
    ) -> i32;
    fn rsf_fixture_xess_present_acquire_semaphore(native: Handle) -> u64;
    fn rsf_fixture_xess_present_end(native: Handle) -> i32;
    fn rsf_fixture_xess_present_submit(native: Handle) -> i32;
    fn rsf_fixture_xess_present_finished_semaphore(native: Handle) -> u64;
    fn rsf_fixture_xess_present_release_views(native: Handle);
    fn rsf_fixture_xess_present_release(native: Handle);

    fn rsf_xessgvk_enable(owner: Handle, enabled: u32) -> i32;
    fn rsf_xessgvk_sleep(owner: Handle, id: u32) -> i32;
    fn rsf_xessgvk_latency(owner: Handle, id: u32, marker: u32) -> i32;
    fn rsf_xessgvk_acquire(
        owner: Handle,
        timeout: u64,
        semaphore: u64,
        fence: u64,
        index: *mut u32,
    ) -> i32;
    fn rsf_xessgvk_prepare(owner: Handle, command: Handle, frame: *const rsf_fgvk_frame) -> i32;
    fn rsf_xessgvk_hudless(owner: Handle, command: Handle, frame: *const rsf_fgvk_frame) -> i32;
    fn rsf_xessgvk_submitted(owner: Handle, command: *const Handle, count: u32);
    fn rsf_xessgvk_present(owner: Handle, info: *const c_void) -> i32;
    fn rsf_xessgvk_status(owner: Handle, status: *mut rsf_fgvk_status) -> i32;
    fn rsf_xessgvk_drain(owner: Handle) -> i32;
    fn rsf_xessgvk_destroy(owner: Handle) -> i32;
}

fn check(result: i32) -> Result<(), i32> {
    if result == 0 { Ok(()) } else { Err(result) }
}

fn frame_record(frame: u32) -> rsf_frame_record {
    // The fixture leaves unspecified scalar camera and provenance fields zero.
    let mut record: rsf_frame_record = unsafe { mem::zeroed() };
    record.struct_size = size_of::<rsf_frame_record>() as u32;
    record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.frame_id = u64::from(frame + 1);
    record.session_id = 1;
    record.view_id = 1;
    record.resource_generation = 1;
    record.flags = if frame == 0 { RSF_FRAME_FLAG_RESET } else { 0 };
    record.frame_time_ms = 16.666667;
    record.render_width = 640;
    record.render_height = 360;
    record.output_width = 1280;
    record.output_height = 720;
    let camera = &mut record.camera;
    camera.struct_size = size_of_val(camera) as u32;
    camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    camera.render_width = 640;
    camera.render_height = 360;
    camera.output_width = 1280;
    camera.output_height = 720;
    camera.near_plane = 1.0;
    camera.far_plane = 100.0;
    camera.vertical_fov_radians = 1.0;
    for i in [0, 5, 10, 15] {
        camera.world_to_view[i] = 1.0;
        camera.view_to_clip[i] = 1.0;
    }
    record
}

struct Receipt {
    completed: u32,
    interpolated: u32,
}

fn present_frames(
    native: Handle,
    owner: Handle,
    chain: u64,
    family: u32,
    receipt: &mut Receipt,
) -> Result<(), i32> {
    unsafe {
        check(rsf_fixture_xess_present_resources(native, owner, family))?;
        check(rsf_xessgvk_enable(owner, 1))?;
        let acquired = rsf_fixture_xess_present_acquire_semaphore(native);
        let finished = [vk::Semaphore::from_raw(
            rsf_fixture_xess_present_finished_semaphore(native),
        )];
        let swapchains = [vk::SwapchainKHR::from_raw(chain)];
        let mut initialized_images = [false; 3];
        for frame in 0..12 {
            check(rsf_fixture_xess_present_idle(native, true))?;
            let id = frame + 1;
            check(rsf_xessgvk_sleep(owner, id))?;
            for marker in 0..3 {
                check(rsf_xessgvk_latency(owner, id, marker))?;
            }
            let mut index = 0;
            check(rsf_xessgvk_acquire(
                owner,
                10_000_000_000,
                acquired,
                0,
                &mut index,
            ))?;
            let initialized = initialized_images
                .get_mut(index as usize)
                .ok_or(vk::Result::ERROR_INITIALIZATION_FAILED.as_raw())?;

            let record = frame_record(frame);
            let mut inputs: rsf_fgvk_frame = mem::zeroed();
            inputs.struct_size = size_of::<rsf_fgvk_frame>() as u32;
            inputs.abi_version = RSF_FGVK_ABI_VERSION;
            inputs.record = &record;
            inputs.motion_scale_x = 640.0;
            inputs.motion_scale_y = 360.0;
            let mut command = null_mut();
            check(rsf_fixture_xess_present_record(
                native,
                frame,
                index,
                *initialized,
                &mut command,
                &mut inputs,
            ))?;
            let handoff = check(rsf_xessgvk_prepare(owner, command, &inputs))
                .and_then(|()| check(rsf_xessgvk_hudless(owner, command, &inputs)));
            let end = rsf_fixture_xess_present_end(native);
            handoff?;
            check(end)?;
            check(rsf_fixture_xess_present_submit(native))?;
            rsf_xessgvk_submitted(owner, &command, 1);
            check(rsf_xessgvk_latency(owner, id, 3))?;
            *initialized = true;

            let indices = [index];
            let present = vk::PresentInfoKHR::default()
                .wait_semaphores(&finished)
                .swapchains(&swapchains)
                .image_indices(&indices);
            let result =
                rsf_xessgvk_present(owner, (&present as *const vk::PresentInfoKHR<'_>).cast());
            let mut status: rsf_fgvk_status = mem::zeroed();
            status.struct_size = size_of::<rsf_fgvk_status>() as u32;
            status.abi_version = RSF_FGVK_ABI_VERSION;
            if rsf_xessgvk_status(owner, &mut status) == 0
                && status.enabled != 0
                && status.last_present_count > 1
            {
                receipt.interpolated += 1;
            }
            check(result)?;
            receipt.completed += 1;
        }
        Ok(())
    }
}

pub(crate) fn run(
    graphics: &rsf_srvk_setup,
    family: u32,
    runtime: *const u16,
    retired: &mut bool,
) -> i32 {
    let mut native = null_mut();
    let mut owner = null_mut();
    let mut chain = 0;
    let created = unsafe {
        rsf_fixture_xess_present_setup(
            graphics,
            family,
            runtime,
            &mut native,
            &mut owner,
            &mut chain,
        )
    };
    if native.is_null() {
        return created;
    }
    let mut receipt = Receipt {
        completed: 0,
        interpolated: 0,
    };
    let result = check(created)
        .and_then(|()| present_frames(native, owner, chain, family, &mut receipt))
        .err()
        .unwrap_or(0);
    let drain = if owner.is_null() {
        0
    } else {
        unsafe { rsf_xessgvk_drain(owner) }
    };
    let release = if drain == 0 {
        unsafe { rsf_fixture_xess_present_release_views(native) };
        unsafe { rsf_xessgvk_destroy(owner) }
    } else {
        -1
    };
    *retired = drain == 0 && release == 0;
    if *retired {
        unsafe {
            rsf_fixture_xess_present_idle(native, false);
            rsf_fixture_xess_present_release(native);
        }
    }
    println!(
        concat!(
            "{{\"presentation_fixture\":true,\"fg_requested\":true,\"create_result\":{},",
            "\"completed_real_frames\":{},\"interpolated_sdk_statuses\":{},\"present_result\":{},",
            "\"drain_result\":{},\"release_result\":{}}}"
        ),
        created, receipt.completed, receipt.interpolated, result, drain, release
    );
    i32::from(result != 0 || !*retired || receipt.completed != 12)
}
