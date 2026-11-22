// SPDX-License-Identifier: MIT
use super::{
    ALL, BOTH, DST, Frame, Gpu, Handle, Image, RANGE, READ, RGBA, UNDEFINED, defaults, extent,
    laid_out, raw,
};
use crate::sys::{
    RSF_SR_FSR4, RSF_SRVK_ABI_VERSION, RSF_SRVK_SPLIT_TRACKED_ABI_VERSION, rsf_fsr4vk_handoff,
};
use ash::vk::{self, Handle as _};
use rsf_game_sys::{
    RSF_FRAME_FLAG_RESET, RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record,
};
use std::{
    ffi::{CStr, CString},
    ptr,
};

use crate::sys::rsf_fsr4sr_fixture_calls as Calls;
use crate::sys::{RSF_FSR4_BRIDGE_ABI_VERSION, rsf_fsr4_bridge_info};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const INVALID: i32 = -1;

fn open() -> Result<(Gpu, u32), i32> {
    let mut gpu = Gpu::open_instance(None, None, c"ReScaleFrame interop device fixture").map_err(
        |error| {
            if let Some(result) = error.downcast_ref::<vk::Result>() {
                result.as_raw()
            } else if error.to_string() == super::NO_LOADER {
                10
            } else {
                11
            }
        },
    )?;
    let devices =
        unsafe { gpu.instance.enumerate_physical_devices() }.map_err(vk::Result::as_raw)?;
    let first = *devices.first().ok_or(12)?;
    gpu.physical = devices
        .into_iter()
        .find(|&physical| {
            let properties = unsafe { gpu.instance.get_physical_device_properties(physical) };
            properties.device_type == vk::PhysicalDeviceType::DISCRETE_GPU
        })
        .unwrap_or(first);
    let physical = gpu.physical;
    let families = unsafe {
        gpu.instance
            .get_physical_device_queue_family_properties(physical)
    };
    let family = families
        .iter()
        .position(|family| family.queue_flags.contains(vk::QueueFlags::GRAPHICS))
        .ok_or(13)? as u32;
    let queues = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(family)
        .queue_priorities(&[1.0])];
    let extensions = [
        ash::khr::external_memory_win32::NAME.as_ptr(),
        ash::khr::external_semaphore_win32::NAME.as_ptr(),
    ];
    let mut timeline =
        vk::PhysicalDeviceTimelineSemaphoreFeatures::default().timeline_semaphore(true);
    let info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queues)
        .enabled_extension_names(&extensions)
        .push_next(&mut timeline);
    gpu.device = Some(
        unsafe { gpu.instance.create_device(physical, &info, None) }.map_err(vk::Result::as_raw)?,
    );
    gpu.queue = unsafe { gpu.device().get_device_queue(family, 0) };
    gpu.memory = unsafe { gpu.instance.get_physical_device_memory_properties(physical) };
    let properties = unsafe { gpu.instance.get_physical_device_properties(physical) };
    let name = unsafe { CStr::from_ptr(properties.device_name.as_ptr()) };
    println!(
        "FSR4 public API fixture: {} (vendor {}, device {})",
        name.to_string_lossy(),
        properties.vendor_id,
        properties.device_id
    );
    Ok((gpu, family))
}

fn begin() -> vk::CommandBufferBeginInfo<'static> {
    vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
}

struct Handoff {
    device: ash::Device,
    continuation: vk::CommandBuffer,
    timeline: vk::Semaphore,
    previous: u64,
    ready: u64,
    done: u64,
    splits: u32,
}

unsafe extern "C" fn split(
    user: Handle,
    commands: Handle,
    timeline: u64,
    previous: u64,
    ready: u64,
    done: u64,
    continuation: *mut Handle,
) -> i32 {
    let state = unsafe { &mut *user.cast::<Handoff>() };
    let commands = vk::CommandBuffer::from_raw(commands as u64);
    let recorded = unsafe {
        state.device.end_command_buffer(commands).and_then(|()| {
            state
                .device
                .begin_command_buffer(state.continuation, &begin())
        })
    };
    if let Err(error) = recorded {
        return error.as_raw();
    }
    state.timeline = vk::Semaphore::from_raw(timeline);
    state.previous = previous;
    state.ready = ready;
    state.done = done;
    state.splits += 1;
    // The host submits both sections only after evaluate returns.
    unsafe { continuation.write(raw(state.continuation)) };
    0
}

impl Handoff {
    fn submit(&self, queue: vk::Queue, producer: vk::CommandBuffer) -> Result<(), i32> {
        let producer = [producer];
        let continuation = [self.continuation];
        let semaphores = [self.timeline];
        let previous = [self.previous];
        let ready = [self.ready];
        let done = [self.done];
        let stages = [ALL];
        let wait_count = usize::from(self.previous != 0);
        let mut first = vk::TimelineSemaphoreSubmitInfo::default()
            .wait_semaphore_values(&previous[..wait_count])
            .signal_semaphore_values(&ready);
        let mut second = vk::TimelineSemaphoreSubmitInfo::default().wait_semaphore_values(&done);
        let submits = [
            vk::SubmitInfo::default()
                .command_buffers(&producer)
                .wait_semaphores(&semaphores[..wait_count])
                .wait_dst_stage_mask(&stages[..wait_count])
                .signal_semaphores(&semaphores)
                .push_next(&mut first),
            vk::SubmitInfo::default()
                .command_buffers(&continuation)
                .wait_semaphores(&semaphores)
                .wait_dst_stage_mask(&stages)
                .push_next(&mut second),
        ];
        unsafe {
            self.device
                .end_command_buffer(self.continuation)
                .map_err(vk::Result::as_raw)?;
            self.device
                .queue_submit(queue, &submits, vk::Fence::null())
                .map_err(vk::Result::as_raw)
        }
    }
}

fn check(result: i32) -> Result<(), i32> {
    if result == 0 { Ok(()) } else { Err(result) }
}

#[derive(Default)]
struct Run {
    width: u32,
    height: u32,
    completed: u32,
    duplicates: u32,
    assertion: i32,
}

impl Run {
    fn exercise(
        &mut self,
        gpu: &mut Gpu,
        state: *mut Handoff,
        family: u32,
        api: &Calls,
        owner: Handle,
        info: &mut rsf_fsr4_bridge_info,
    ) -> Result<(), i32> {
        unsafe {
            check(api.plan.unwrap()(owner, &mut self.width, &mut self.height))?;
            check(api.info.unwrap()(owner, info))?;
        }
        let width = self.width;
        let height = self.height;
        let render = extent(width as usize, height as usize);
        let usage = BOTH | vk::ImageUsageFlags::SAMPLED;
        let image_error = |error: Box<dyn std::error::Error>| {
            error
                .downcast_ref::<vk::Result>()
                .map_or(vk::Result::ERROR_OUT_OF_DEVICE_MEMORY.as_raw(), |code| {
                    code.as_raw()
                })
        };
        let images = [
            gpu.image(render, RGBA, usage).map_err(image_error)?,
            gpu.image(render, vk::Format::R32_SFLOAT, usage)
                .map_err(image_error)?,
            gpu.image(render, vk::Format::R16G16_SFLOAT, usage)
                .map_err(image_error)?,
            gpu.image(extent(WIDTH as usize, HEIGHT as usize), RGBA, usage)
                .map_err(image_error)?,
        ];
        let pool = vk::CommandPoolCreateInfo::default()
            .queue_family_index(family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        gpu.pool =
            unsafe { gpu.device().create_command_pool(&pool, None) }.map_err(vk::Result::as_raw)?;
        let alloc = vk::CommandBufferAllocateInfo::default()
            .command_pool(gpu.pool)
            .command_buffer_count(2);
        let commands =
            unsafe { gpu.device().allocate_command_buffers(&alloc) }.map_err(vk::Result::as_raw)?;
        gpu.commands = commands[0];
        unsafe { (*state).continuation = commands[1] };
        let producer = raw(commands[0]);
        let consumer = raw(commands[1]);

        for id in 1..=6 {
            let device = gpu.device();
            unsafe {
                device.device_wait_idle().map_err(vk::Result::as_raw)?;
                for &commands in &commands {
                    device
                        .reset_command_buffer(commands, vk::CommandBufferResetFlags::empty())
                        .map_err(vk::Result::as_raw)?;
                }
                device
                    .begin_command_buffer(gpu.commands, &begin())
                    .map_err(vk::Result::as_raw)?;
            }
            if id == 1 {
                seed(gpu, images);
            }
            let mut record = rsf_frame_record {
                struct_size: size_of::<rsf_frame_record>() as u32,
                abi_version: RSF_GAME_FRAME_ABI_VERSION,
                frame_id: id as u64,
                session_id: 1,
                view_id: 1,
                resource_generation: 1,
                render_width: width,
                render_height: height,
                output_width: WIDTH,
                output_height: HEIGHT,
                frame_time_ms: 16.666667,
                flags: if id == 1 { RSF_FRAME_FLAG_RESET } else { 0 },
                ..unsafe { std::mem::zeroed() }
            };
            let camera = &mut record.camera;
            camera.struct_size = std::mem::size_of_val(camera) as u32;
            camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
            camera.render_width = width;
            camera.render_height = height;
            camera.output_width = WIDTH;
            camera.output_height = HEIGHT;
            camera.near_plane = 0.1;
            camera.vertical_fov_radians = 1.0;
            camera.depth_inverted = 1;

            let mut frame = Frame {
                struct_size: size_of::<Frame>() as u32,
                abi_version: RSF_SRVK_ABI_VERSION,
                record: &record,
                color: images[0],
                depth: images[1],
                motion: images[2],
                output: laid_out(images[3], DST),
                motion_scale_x: width as f32,
                motion_scale_y: height as f32,
                pre_exposure: 1.0,
                sharpening: defaults(),
                ..unsafe { std::mem::zeroed() }
            };
            frame.sharpening.mode = if id == 6 { 4 } else { (id - 1) % 4 };
            frame.sharpening.sharpness = 0.7;

            unsafe { api.retire.unwrap()(owner, consumer) };
            let before_splits = unsafe { (*state).splits };
            let evaluate = |frame: &Frame, next: &mut Handle| unsafe {
                api.evaluate.unwrap()(owner, producer, frame, next)
            };
            let mut next = ptr::null_mut();
            frame.color.copy_source = 0;
            let capability = evaluate(&frame, &mut next);
            if capability != INVALID
                || next != producer
                || unsafe { (*state).splits } != before_splits
            {
                self.assertion = 20;
                return Err(capability);
            }
            frame.color.copy_source = 1;
            if id > 1 {
                let other_view = rsf_frame_record {
                    view_id: 2,
                    ..record
                };
                frame.record = &other_view;
                let view = evaluate(&frame, &mut next);
                if view != INVALID
                    || next != producer
                    || unsafe { (*state).splits } != before_splits
                {
                    self.assertion = 21;
                    return Err(view);
                }
                frame.record = &record;
            }
            let provider = evaluate(&frame, &mut next);
            if next != consumer {
                return Err(if provider == 0 { INVALID } else { provider });
            }
            if unsafe { api.destroy.unwrap()(owner) } != vk::Result::NOT_READY.as_raw() {
                self.assertion = 22;
                return Err(INVALID);
            }
            if evaluate(&frame, &mut ptr::null_mut()) == INVALID {
                self.duplicates += 1;
            }
            if provider != 0 {
                unsafe {
                    device.cmd_clear_color_image(
                        commands[1],
                        vk::Image::from_raw(images[3].image),
                        DST,
                        &vk::ClearColorValue {
                            float32: [0.0, 0.0, 0.0, 1.0],
                        },
                        &[RANGE],
                    );
                }
            }
            unsafe {
                (*state).submit(gpu.queue, commands[0])?;
                let submitted = api.submitted.unwrap();
                if submitted(owner, consumer) != INVALID {
                    self.assertion = 23;
                    return Err(INVALID);
                }
                check(submitted(owner, producer))?;
                if api.destroy.unwrap()(owner) != vk::Result::NOT_READY.as_raw() {
                    self.assertion = 24;
                    return Err(INVALID);
                }
                check(submitted(owner, consumer))?;
                device.device_wait_idle().map_err(vk::Result::as_raw)?;
            }
            check(provider)?;
            self.completed += 1;
        }
        Ok(())
    }
}

fn seed(gpu: &Gpu, images: [Image; 4]) {
    for image in images {
        gpu.transition(image, UNDEFINED, DST);
    }
    for (image, clear) in
        images
            .into_iter()
            .zip([[0.2, 0.4, 0.8, 1.0], [0.75, 0.0, 0.0, 0.0], [0.0; 4]])
    {
        gpu.fill(image, clear);
        gpu.transition(image, DST, READ);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_fsr4sr_device(
    api: &Calls,
    path: *const u16,
    length: usize,
) -> i32 {
    let (mut gpu, family) = match open() {
        Ok(device) => device,
        Err(result) => {
            println!("{{\"device_create\":{result}}}");
            return 5;
        }
    };
    let path = unsafe { std::slice::from_raw_parts(path, length) };
    let directory =
        CString::new(String::from_utf16_lossy(path)).expect("wide argv contains no NUL");
    let mut graphics = gpu.setup(RSF_SR_FSR4, &directory);
    graphics.output_width = WIDTH;
    graphics.output_height = HEIGHT;
    graphics.depth_infinite = 1;
    let mut state = Box::new(Handoff {
        device: gpu.device().clone(),
        continuation: vk::CommandBuffer::null(),
        timeline: vk::Semaphore::null(),
        previous: 0,
        ready: 0,
        done: 0,
        splits: 0,
    });
    let state_ptr = &raw mut *state;
    let handoff = rsf_fsr4vk_handoff {
        struct_size: size_of::<rsf_fsr4vk_handoff>() as u32,
        abi_version: RSF_SRVK_SPLIT_TRACKED_ABI_VERSION,
        timeline_enabled: 1,
        queue_family: family,
        user: state_ptr.cast(),
        split_record: Some(split),
    };
    let mut owner = ptr::null_mut();
    let create = unsafe { api.create.unwrap()(&graphics, &handoff, &mut owner) };
    let mut info = rsf_fsr4_bridge_info {
        struct_size: size_of::<rsf_fsr4_bridge_info>() as u32,
        abi_version: RSF_FSR4_BRIDGE_ABI_VERSION,
        ..unsafe { std::mem::zeroed() }
    };
    let mut run = Run::default();
    let result = check(create)
        .and_then(|()| run.exercise(&mut gpu, state_ptr, family, api, owner, &mut info))
        .err()
        .unwrap_or(0);
    // A failed destroy assertion may mean the owner has already been released.
    if run.assertion != 0 {
        std::mem::forget((gpu, state, directory));
        return run.assertion;
    }
    let release = unsafe { api.destroy.unwrap()(owner) };
    println!(
        concat!(
            "{{\"contract_checks\":\"passed\",\"device_test\":true,\"create_result\":{},\"evaluate_result\":{},",
            "\"completed_frames\":{},\"deferred_splits\":{},\"duplicate_frames_refused\":{},\"render_width\":{},\"render_height\":{},",
            "\"provider_version\":{},\"vendor_id\":{},\"int8_compatibility\":{},\"release_result\":{}}}"
        ),
        create,
        result,
        run.completed,
        state.splits,
        run.duplicates,
        run.width,
        run.height,
        info.provider_version,
        info.vendor_id,
        info.int8_compatibility,
        release
    );
    if release != 0 {
        // The retained bridge still borrows the Vulkan resources and split state.
        std::mem::forget((gpu, state, directory));
    }
    i32::from(result != 0 || release != 0 || run.completed != 6 || run.duplicates != 6)
}
