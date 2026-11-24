// SPDX-License-Identifier: MIT
use crate::sys::rsf_srvk_setup;
use crate::sys::rsf_xessgvk_options;
use crate::sys::{RSF_FGVK_ABI_VERSION, rsf_fgvk_frame, rsf_fgvk_setup, rsf_fgvk_status};
use crate::xess_present_gpu::{self as gpu, Queue, Resources};
use ash::{
    Device,
    vk::{self, Handle as _},
};
use rsf_game_sys::{RSF_FRAME_FLAG_RESET, RSF_GAME_CAMERA_ABI_VERSION, rsf_frame_record};
use std::{
    ffi::c_void,
    mem,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{HINSTANCE, HWND},
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG,
        PM_REMOVE, PeekMessageW, RegisterClassW, SW_SHOWNORMAL, SetForegroundWindow, ShowWindow,
        TranslateMessage, UnregisterClassW, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    },
};

type Handle = *mut c_void;

const WINDOW_CLASS: *const u16 = windows_sys::w!("RsfXeSSInteropFixture");

struct Window {
    handle: HWND,
    instance: HINSTANCE,
}

impl Window {
    fn open() -> Result<Self, i32> {
        let instance = unsafe { GetModuleHandleW(null()) };
        let class = WNDCLASSW {
            lpfnWndProc: Some(DefWindowProcW),
            hInstance: instance,
            lpszClassName: WINDOW_CLASS,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err(100);
        }
        let handle = unsafe {
            CreateWindowExW(
                0,
                WINDOW_CLASS,
                windows_sys::w!("ReScaleFrame XeSS interop fixture"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1296,
                759,
                null_mut(),
                null_mut(),
                instance,
                null(),
            )
        };
        if handle.is_null() {
            unsafe { UnregisterClassW(WINDOW_CLASS, instance) };
            return Err(101);
        }
        unsafe {
            ShowWindow(handle, SW_SHOWNORMAL);
            SetForegroundWindow(handle);
        }
        Ok(Self { handle, instance })
    }

    fn pump() {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.handle);
            UnregisterClassW(WINDOW_CLASS, self.instance);
        }
    }
}

unsafe extern "C" {
    fn rsf_xessgvk_create(
        setup: *const rsf_fgvk_setup,
        options: *const rsf_xessgvk_options,
        owner: *mut Handle,
        chain: *mut u64,
    ) -> i32;
    fn rsf_xessgvk_images(owner: Handle, count: *mut u32, images: *mut u64) -> i32;
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
    let mut record = rsf_frame_record {
        resource_generation: 1,
        flags: if frame == 0 { RSF_FRAME_FLAG_RESET } else { 0 },
        frame_time_ms: 16.666667,
        ..rsf_test_support::frame_record(1, u64::from(frame + 1), 1, [640, 360], [1280, 720])
    };
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
    device: &Device,
    queue: vk::Queue,
    resources: &Resources,
    owner: Handle,
    chain: u64,
    receipt: &mut Receipt,
) -> Result<(), i32> {
    unsafe {
        check(rsf_xessgvk_enable(owner, 1))?;
        let acquired = resources.acquired.as_raw();
        let finished = [resources.finished];
        let swapchains = [vk::SwapchainKHR::from_raw(chain)];
        let mut initialized_images = [false; 3];
        for frame in 0..12 {
            Window::pump();
            device.device_wait_idle().map_err(vk::Result::as_raw)?;
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
            let image = index as usize;
            resources
                .start_frame(device, frame, image, *initialized)
                .map_err(vk::Result::as_raw)?;
            [inputs.depth, inputs.motion, inputs.hudless] = resources.inputs(device, image);
            let command = resources.commands.as_raw() as Handle;
            let handoff = check(rsf_xessgvk_prepare(owner, command, &inputs))
                .and_then(|()| check(rsf_xessgvk_hudless(owner, command, &inputs)));
            let end = device.end_command_buffer(resources.commands);
            handoff?;
            end.map_err(vk::Result::as_raw)?;
            resources
                .submit(device, queue)
                .map_err(vk::Result::as_raw)?;
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
    device: &Device,
    family: u32,
    memory: vk::PhysicalDeviceMemoryProperties,
    runtime: *const u16,
    retired: &mut bool,
) -> i32 {
    let window = match Window::open() {
        Ok(window) => window,
        Err(code) => return code,
    };
    let mut queue = Box::new(Queue {
        handle: unsafe { device.get_device_queue(family, 0) },
        submit: device.fp_v1_0().queue_submit,
    });
    let mut owner = null_mut();
    let mut chain = 0;
    let swapchain = vk::SwapchainCreateInfoKHR::default()
        .min_image_count(3)
        .image_format(gpu::COLOR)
        .image_color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
        .image_extent(gpu::OUTPUT)
        .image_array_layers(1)
        .image_usage(
            vk::ImageUsageFlags::COLOR_ATTACHMENT
                | vk::ImageUsageFlags::TRANSFER_DST
                | vk::ImageUsageFlags::TRANSFER_SRC,
        )
        .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
        .present_mode(vk::PresentModeKHR::FIFO);
    let setup = rsf_fgvk_setup {
        struct_size: size_of::<rsf_fgvk_setup>() as u32,
        abi_version: RSF_FGVK_ABI_VERSION,
        graphics: *graphics,
        queues: [queue.handle.as_raw() as Handle; 4],
        queue_family: family,
        create_info: (&swapchain as *const vk::SwapchainCreateInfoKHR<'_>).cast(),
        allocator: std::ptr::null(),
        graphics_submit: Some(gpu::submit),
        lock_queues: Some(gpu::lock),
        unlock_queues: Some(gpu::lock),
        queue_user: (&raw mut *queue).cast(),
    };
    let options = rsf_xessgvk_options {
        struct_size: size_of::<rsf_xessgvk_options>() as u32,
        abi_version: 1,
        window: window.handle,
        timeline_enabled: 1,
        generated_count: 1,
        intel_runtime_directory: runtime,
    };
    let created = unsafe { rsf_xessgvk_create(&setup, &options, &mut owner, &mut chain) };
    let mut resources = Resources::default();
    let mut receipt = Receipt {
        completed: 0,
        interpolated: 0,
    };
    let result = check(created)
        .and_then(|()| {
            resources
                .allocate(device, family, &memory)
                .map_err(vk::Result::as_raw)?;
            let mut images = [0; 3];
            let mut count = images.len() as u32;
            check(unsafe { rsf_xessgvk_images(owner, &mut count, images.as_mut_ptr()) })?;
            if count != images.len() as u32 || images.contains(&0) {
                return Err(vk::Result::ERROR_INITIALIZATION_FAILED.as_raw());
            }
            resources
                .attach_images(device, images)
                .map_err(vk::Result::as_raw)?;
            present_frames(device, queue.handle, &resources, owner, chain, &mut receipt)
        })
        .err()
        .unwrap_or(0);
    let drain = if owner.is_null() {
        0
    } else {
        unsafe { rsf_xessgvk_drain(owner) }
    };
    let release = if drain == 0 {
        resources.release_views(device);
        unsafe { rsf_xessgvk_destroy(owner) }
    } else {
        -1
    };
    *retired = drain == 0 && release == 0;
    if *retired {
        let _ = unsafe { device.device_wait_idle() };
        resources.release(device);
        drop(window);
    } else {
        // The owner can still retain the callback user and Vulkan resources.
        mem::forget(queue);
        mem::forget(window);
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
