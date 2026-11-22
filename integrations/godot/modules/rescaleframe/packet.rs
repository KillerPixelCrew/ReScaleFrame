// SPDX-License-Identifier: MIT
//! Driver callback packet assembly and synchronous execution.
use core::ffi::c_void;
use core::mem::{MaybeUninit, size_of};
use core::ptr;

use crate::frame_event_abi::{RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_SR};
use crate::packet_abi::{
    RSF_FGVK_ABI_VERSION, RSF_GODOT_ABI_VERSION, RSF_SRVK_ABI_VERSION, rsf_fgvk_setup,
    rsf_game_render_pass, rsf_godot_driver_packet, rsf_godot_packet_calls,
    rsf_godot_packet_sources, rsf_godot_swapchain_calls, rsf_godot_swapchain_release_calls,
    rsf_godot_vk_image, rsf_godot_vk_packet, rsf_srvk_setup,
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
        if calls.native_wsi.unwrap_unchecked()(context) != 0
            || calls.timeline.unwrap_unchecked()(context) == 0
            || calls.queue_count.unwrap_unchecked()(context) < 4
        {
            return 0;
        }
        let mut chain = 0;
        let created = calls.create.unwrap_unchecked()(setup.cast(), owner, &mut chain) != 0;
        // A refused proxy can still leave an owner that the engine has to retire.
        if created || !(*owner).is_null() {
            calls.install.unwrap_unchecked()(context, chain);
        }
        u32::from(created)
    }
}

/// # Safety
/// The chain, its owner field and all callbacks must be valid through this synchronous call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_swapchain_release(
    calls: *const rsf_godot_swapchain_release_calls,
    chain: *mut c_void,
    owner: *const *mut c_void,
) -> u32 {
    let calls = unsafe { &*calls };
    if unsafe { calls.release_capture.unwrap_unchecked()(chain) } == 0 {
        return 0;
    }
    let owner = unsafe { *owner };
    if owner.is_null() {
        return 1;
    }
    if unsafe { calls.destroy_proxy.unwrap_unchecked()(owner) } == 0 {
        return 0;
    }
    unsafe { calls.clear_proxy.unwrap_unchecked()(chain) };
    1
}

/// # Safety
/// `record` and `textures` must be initialized and `out` writable. Godot must keep the
/// scene textures and the required callback arguments live for the complete call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_packet_prepare(
    record: *const Record,
    sources: *const rsf_godot_packet_sources,
    context: *mut c_void,
    textures: *const [u64; 4],
    out: *mut rsf_godot_driver_packet,
) {
    let (record, sources, [color, depth, motion, output]) =
        unsafe { (&*record, &*sources, *textures) };
    let mut payload = rsf_godot_vk_packet {
        device: unsafe { sources.device.unwrap_unchecked()(context) },
        pass: record.public_pass(),
        ..Default::default()
    };
    let queue = if record.driver == DRIVER_VULKAN {
        let final_scene = record.role == RSF_GAME_RENDER_FINAL_SCENE;
        let camera = &payload.pass.camera;
        let render = [camera.render_width, camera.render_height];
        let target = [camera.output_width, camera.output_height];
        let describe = |rid: u64, [width, height]: [u32; 2]| {
            let mut image = rsf_godot_vk_image::default();
            if rid != 0 {
                unsafe { sources.vk_image.unwrap_unchecked()(context, rid, &mut image) };
            }
            image.width = width;
            image.height = height;
            image
        };
        payload.struct_size = size_of::<rsf_godot_vk_packet>() as u32;
        payload.abi_version = RSF_GODOT_ABI_VERSION;
        if record.role == RSF_GAME_RENDER_SR {
            unsafe {
                sources.vk_context.unwrap_unchecked()(
                    context,
                    &mut payload.instance,
                    &mut payload.physical_device,
                )
            };
        }
        unsafe {
            sources.vk_procs.unwrap_unchecked()(
                &mut payload.instance_proc,
                &mut payload.device_proc,
            )
        };
        payload.color = describe(color, if final_scene { target } else { render });
        if !final_scene {
            payload.depth = describe(depth, render);
            payload.motion = describe(motion, render);
            payload.output = describe(output, target);
        }
        ptr::null_mut()
    } else {
        let queue = unsafe { sources.queue.unwrap_unchecked()(context) };
        let native = |rid| {
            if rid == 0 {
                ptr::null_mut()
            } else {
                unsafe { sources.texture.unwrap_unchecked()(context, rid) as usize as *mut c_void }
            }
        };
        payload.pass.color_input = native(color);
        payload.pass.depth = native(depth);
        payload.pass.motion = native(motion);
        payload.pass.color_output = native(output);
        queue
    };
    unsafe {
        out.write(rsf_godot_driver_packet {
            driver: record.driver,
            queue,
            payload,
        })
    };
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
    if packet.driver == DRIVER_VULKAN {
        let commands = unsafe { calls.begin_vulkan.unwrap_unchecked()(context) };
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
            let commands = calls.begin_d3d12.unwrap_unchecked()(context);
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
