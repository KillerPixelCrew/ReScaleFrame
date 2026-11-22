// SPDX-License-Identifier: MIT
//! Driver callback packet assembly and synchronous execution.
use core::ffi::c_void;
use core::mem::{MaybeUninit, size_of};
use core::ptr;

use crate::frame_event_abi::RSF_GAME_RENDER_FINAL_SCENE;
use crate::packet_abi::{
    RSF_FGVK_ABI_VERSION, RSF_GODOT_ABI_VERSION, RSF_GODOT_CHAIN_CREATED, RSF_GODOT_CHAIN_REFUSED,
    RSF_GODOT_CHAIN_SKIPPED, RSF_SRVK_ABI_VERSION, rsf_fgvk_setup, rsf_game_render_pass,
    rsf_godot_driver_packet, rsf_godot_packet_calls, rsf_godot_packet_facts,
    rsf_godot_swapchain_calls, rsf_godot_vk_image, rsf_godot_vk_packet, rsf_srvk_setup,
};
use crate::pass::Record;
use crate::primary::DRIVER_VULKAN;

/// # Safety
/// `out` must be writable, aligned storage for one pass.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_execution_pass(
    role: u32,
    session: u64,
    source: u64,
    window: u64,
    out: *mut rsf_game_render_pass,
) {
    unsafe {
        out.write(rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            role,
            session_id: session,
            source_frame_id: source,
            window_key: window,
            ..Default::default()
        });
    }
}

/// # Safety
/// All pointers must remain valid for the call. Every callback must be nonnull.
/// `setup` holds Godot's initialized creation facts and remains writable here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_swapchain_setup(
    setup: *mut rsf_fgvk_setup,
    calls: *const rsf_godot_swapchain_calls,
    context: *mut c_void,
    owner: *mut *mut c_void,
    chain: *mut u64,
) -> u32 {
    // Godot owns the storage and the Vulkan creation lease for these synchronous calls.
    let calls = unsafe { &*calls };
    unsafe {
        (*setup).struct_size = size_of::<rsf_fgvk_setup>() as u32;
        (*setup).abi_version = RSF_FGVK_ABI_VERSION;
        (*setup).graphics.struct_size = size_of::<rsf_srvk_setup>() as u32;
        (*setup).graphics.abi_version = RSF_SRVK_ABI_VERSION;
        (*setup).graphics.depth_inverted = 1;
        (calls.capture.unwrap_unchecked())(setup.cast());
        if (calls.allow_proxy.unwrap_unchecked())(context) == 0 {
            return RSF_GODOT_CHAIN_SKIPPED;
        }
        if (calls.create.unwrap_unchecked())(setup.cast(), owner, chain) != 0 {
            RSF_GODOT_CHAIN_CREATED
        } else {
            RSF_GODOT_CHAIN_REFUSED
        }
    }
}

/// # Safety
/// `record` and `facts` must be initialized. `out` must be writable, aligned packet storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_packet_prepare(
    record: *const Record,
    facts: *const rsf_godot_packet_facts,
    out: *mut rsf_godot_driver_packet,
) {
    // SAFETY: the rendering server supplies initialized inputs and writable output.
    let (record, facts) = unsafe { (&*record, &*facts) };
    let mut payload = rsf_godot_vk_packet {
        device: facts.device,
        pass: record.public_pass(),
        ..Default::default()
    };
    if record.driver == DRIVER_VULKAN {
        let final_scene = record.role == RSF_GAME_RENDER_FINAL_SCENE;
        let camera = &payload.pass.camera;
        let render = [camera.render_width, camera.render_height];
        let output = [camera.output_width, camera.output_height];
        let describe = |mut image: rsf_godot_vk_image, [width, height]| {
            image.width = width;
            image.height = height;
            image
        };
        payload.struct_size = size_of::<rsf_godot_vk_packet>() as u32;
        payload.abi_version = RSF_GODOT_ABI_VERSION;
        payload.instance = facts.instance;
        payload.physical_device = facts.physical_device;
        payload.instance_proc = facts.instance_proc;
        payload.device_proc = facts.device_proc;
        payload.color = describe(facts.vk_color, if final_scene { output } else { render });
        if !final_scene {
            payload.depth = describe(facts.vk_depth, render);
            payload.motion = describe(facts.vk_motion, render);
            payload.output = describe(facts.vk_output, output);
        }
    } else {
        payload.pass.color_input = facts.color;
        payload.pass.color_output = facts.output;
        payload.pass.depth = facts.depth;
        payload.pass.motion = facts.motion;
    }
    unsafe {
        out.write(rsf_godot_driver_packet {
            driver: record.driver,
            queue: facts.queue,
            payload,
        });
    }
}

/// # Safety
/// The packet, function table, callback context and resource leases must live through
/// the synchronous callback. The functions for the selected driver must be nonnull.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_packet_execute(
    packet: *const rsf_godot_driver_packet,
    calls: *const rsf_godot_packet_calls,
    context: *mut c_void,
) {
    // SAFETY: a single Godot callback owns the packet, driver context and resource leases.
    let (packet, calls) = unsafe { (&*packet, &*calls) };
    let commands = unsafe { calls.begin.unwrap_unchecked()(context, packet.driver) };
    if packet.driver == DRIVER_VULKAN {
        if commands.is_null() {
            return;
        }
        let vk = &packet.payload;
        let mut handoff = MaybeUninit::uninit();
        if unsafe { calls.handoff.unwrap_unchecked()(context, handoff.as_mut_ptr()) } != 0 {
            let mut continuation = ptr::null_mut();
            // GraphScope installs its continuation even when the provider needs a fallback.
            unsafe {
                calls.render_handoff.unwrap_unchecked()(
                    commands,
                    vk,
                    handoff.as_ptr(),
                    &mut continuation,
                )
            };
        } else {
            unsafe { calls.render_vulkan.unwrap_unchecked()(commands, vk) };
        }
    } else {
        unsafe {
            calls.render_d3d12.unwrap_unchecked()(
                packet.payload.device,
                packet.queue,
                commands,
                &packet.payload.pass,
            );
            calls.end_d3d12.unwrap_unchecked()(context);
        }
    }
}
