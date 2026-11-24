// SPDX-License-Identifier: MIT
use super::*;
use crate::vulkan::handle;
use ash::vk::Handle as _;
use rsf_game_sys::{RSF_FRAME_FLAG_NO_FG, RSF_FRAME_FLAG_NO_SR};

const TRANSFER_SRC: vk::ImageUsageFlags = vk::ImageUsageFlags::TRANSFER_SRC;
const SWAPCHAIN_IMAGES: usize = 16;

unsafe extern "C" {
    fn rsf_core_capture_vk_create(command_completion: u32) -> Handle;
    fn rsf_core_capture_vk_initialize(
        owner: Handle,
        device: Handle,
        proc: Option<VkProc>,
        memory: *const vk::PhysicalDeviceMemoryProperties,
    ) -> i32;
    fn rsf_core_capture_vk_wants(owner: Handle, frame: u64) -> i32;
    fn rsf_core_capture_vk_record(
        owner: Handle,
        commands: Handle,
        image: *const rsf_srvk_image,
        identity: *const rsf_frame_record,
        role: *const c_char,
        reset: u32,
        core_source: u32,
        ticket: *mut u64,
        group: u64,
    ) -> i32;
    fn rsf_core_capture_vk_retire(owner: Handle, commands: Handle);
    fn rsf_core_capture_vk_flush(owner: Handle, ready_only: u32, commands: Handle) -> i32;
    fn rsf_core_capture_vk_discard_all(owner: Handle);
    fn rsf_core_capture_vk_destroy(owner: Handle);
    fn rsf_core_capture_vk_pending(owner: Handle) -> u64;
}

#[derive(Default)]
pub(super) struct Capture {
    owner: *mut rsf_input_dump_vk,
    stages: Handle,
    setup: rsf_input_dump_vk_setup,
    format: u32,
    width: u32,
    height: u32,
    usage: vk::ImageUsageFlags,
    chain: vk::SwapchainKHR,
    record: Option<rsf_frame_record>,
}

pub(super) fn readable_create(
    setup: &rsf_fgvk_setup,
) -> Option<vk::SwapchainCreateInfoKHR<'static>> {
    features().capture.create?;
    let info = setup
        .create_info
        .cast::<vk::SwapchainCreateInfoKHR<'static>>();
    if info.is_null() {
        return None;
    }
    let mut info = unsafe { info.read() };
    let graphics = &setup.graphics;
    if let Some(instance_proc) = graphics.instance_proc {
        let get = unsafe {
            std::mem::transmute::<
                Option<VkFunction>,
                Option<vk::PFN_vkGetPhysicalDeviceSurfaceCapabilitiesKHR>,
            >(instance_proc(
                graphics.instance,
                c"vkGetPhysicalDeviceSurfaceCapabilitiesKHR".as_ptr(),
            ))
        };
        let mut caps = vk::SurfaceCapabilitiesKHR::default();
        if get.is_some_and(|get| unsafe {
            get(handle(graphics.physical_device), info.surface, &mut caps)
        } == vk::Result::SUCCESS)
            && caps.supported_usage_flags.contains(TRANSFER_SRC)
        {
            info.image_usage |= TRANSFER_SRC;
        }
    }
    Some(info)
}

impl Capture {
    pub(super) fn user(&self) -> Handle {
        self.setup.user
    }

    pub(super) fn setup(&mut self, setup: &rsf_fgvk_setup) {
        let Some(create) = features().capture.create else {
            return;
        };
        if !self.owner.is_null() && self.setup.user != setup.queue_user {
            return;
        }
        let Some(info) = (unsafe {
            setup
                .create_info
                .cast::<vk::SwapchainCreateInfoKHR<'static>>()
                .as_ref()
        }) else {
            return;
        };
        self.setup = rsf_input_dump_vk_setup {
            struct_size: size_of::<rsf_input_dump_vk_setup>() as u32,
            abi_version: 1,
            graphics: rsf_srvk_setup {
                runtime_directory_utf8: ptr::null(),
                ..setup.graphics
            },
            queue: setup.queues[0],
            queue_family: setup.queue_family,
            submit: setup.graphics_submit,
            user: setup.queue_user,
        };
        if self.owner.is_null() {
            let _ = unsafe { create(&self.setup, &mut self.owner) };
        }
        self.format = info.image_format.as_raw() as u32;
        self.width = info.image_extent.width;
        self.height = info.image_extent.height;
        self.usage = info.image_usage;
        if let Some(proc) = setup.graphics.instance_proc {
            let properties = unsafe {
                std::mem::transmute::<
                    Option<VkFunction>,
                    Option<vk::PFN_vkGetPhysicalDeviceMemoryProperties>,
                >(proc(
                    setup.graphics.instance,
                    c"vkGetPhysicalDeviceMemoryProperties".as_ptr(),
                ))
            };
            if let Some(properties) = properties {
                let mut memory = vk::PhysicalDeviceMemoryProperties::default();
                unsafe { properties(handle(setup.graphics.physical_device), &mut memory) };
                if self.stages.is_null() {
                    self.stages = unsafe { rsf_core_capture_vk_create(0) };
                }
                let _ = unsafe {
                    rsf_core_capture_vk_initialize(
                        self.stages,
                        setup.graphics.device,
                        setup.graphics.device_proc,
                        &memory,
                    )
                };
            }
        }
    }

    pub(super) fn release(&mut self, user: Handle) -> i32 {
        if self.owner.is_null() || self.setup.user != user {
            return backend_result::OK;
        }
        let ops = &features().capture;
        let (Some(drain), Some(destroy)) = (ops.drain, ops.destroy) else {
            return backend_result::NOT_READY;
        };
        let joined = unsafe { drain(self.owner) };
        if joined != backend_result::OK && joined != RSF_INPUT_DUMP_VK_DEVICE_LOST {
            return joined;
        }
        let stopped = unsafe { destroy(self.owner) };
        if stopped != backend_result::OK {
            return stopped;
        }
        if joined == backend_result::OK {
            let _ = unsafe { rsf_core_capture_vk_flush(self.stages, 0, ptr::null_mut()) };
        } else {
            unsafe { rsf_core_capture_vk_discard_all(self.stages) };
        }
        self.owner = ptr::null_mut();
        self.chain = vk::SwapchainKHR::null();
        self.record = None;
        self.setup = rsf_input_dump_vk_setup::default();
        backend_result::OK
    }

    pub(super) fn stop(&mut self) {
        // Leave any still-borrowed recordings with their owner.
        if self.owner.is_null() && unsafe { rsf_core_capture_vk_pending(self.stages) } == 0 {
            unsafe {
                rsf_core_capture_vk_destroy(std::mem::replace(&mut self.stages, ptr::null_mut()))
            };
        }
    }

    pub(super) fn retire(&self, commands: Handle) {
        if features().capture.create.is_some() {
            unsafe { rsf_core_capture_vk_retire(self.stages, commands) };
        }
    }

    pub(super) fn render(
        &mut self,
        commands: Handle,
        packet: &rsf_godot_vk_packet,
        record: rsf_frame_record,
        backend: u32,
    ) {
        if features().capture.create.is_none() {
            return;
        }
        self.record = Some(record);
        if packet.pass.role == RSF_GAME_RENDER_SR {
            let _ = unsafe { rsf_input_dump_begin_record(&record, backend) };
        }
        // Poll previously captured snapshots even after the selected frame.
        let _ = unsafe { rsf_core_capture_vk_wants(self.stages, record.frame_id) };
        if unsafe { rsf_input_dump_wants(&record) } == 0 {
            return;
        }
        unsafe { rsf_input_dump_pass_metadata(&packet.pass) };
        if packet.pass.role == RSF_GAME_RENDER_FINAL_SCENE {
            let _ = unsafe {
                rsf_core_capture_vk_record(
                    self.stages,
                    commands,
                    &image(packet, &packet.color, SHADER_READ),
                    &record,
                    c"hudless_color".as_ptr(),
                    record.flags,
                    1,
                    &mut 0,
                    0,
                )
            };
        }
    }

    pub(super) fn can_present(&self) -> bool {
        !self.owner.is_null()
    }

    /// Returns whether the capture should emit a failure diagnostic outside the state lock.
    pub(super) fn present(
        &mut self,
        owner: *mut rsf_fgvk,
        info: *const c_void,
        result: i32,
        begin: bool,
        view: &VkView,
        frame: u64,
    ) -> bool {
        let Some(snapshot) = features().capture.snapshot else {
            return false;
        };
        let record = self
            .record
            .filter(|record| record.frame_id == frame)
            .unwrap_or_else(|| rsf_frame_record {
                struct_size: size_of::<rsf_frame_record>() as u32,
                abi_version: RSF_GAME_FRAME_ABI_VERSION,
                session_id: view.session_id,
                frame_id: frame,
                output_width: self.width,
                output_height: self.height,
                flags: RSF_FRAME_FLAG_NO_SR | RSF_FRAME_FLAG_NO_FG,
                ..empty_frame_record()
            });
        if !begin {
            unsafe { rsf_input_dump_complete(&record, result) };
            return false;
        }
        let _ = unsafe { rsf_input_dump_begin_record(&record, view.settings.sr_backend) };
        if unsafe { rsf_input_dump_wants(&record) } == 0 {
            return false;
        }

        let info = unsafe { &*info.cast::<vk::PresentInfoKHR<'_>>() };
        if info.swapchain_count != 1
            || info.p_swapchains.is_null()
            || info.p_image_indices.is_null()
        {
            return false;
        }
        let chain = unsafe { *info.p_swapchains };
        if owner.is_null() && self.chain != vk::SwapchainKHR::null() && self.chain != chain {
            return false;
        }
        self.chain = chain;
        let index = unsafe { *info.p_image_indices } as usize;
        let Some((value, layout)) = self.present_image(owner, index) else {
            return false;
        };
        let image = rsf_srvk_image {
            device: self.setup.graphics.device,
            image: value,
            format: self.format,
            width: self.width,
            height: self.height,
            layout,
            copy_source: u32::from(self.usage.contains(TRANSFER_SRC)),
        };
        unsafe {
            snapshot(self.owner, &record, &image, c"final_color".as_ptr()) != backend_result::OK
        }
    }

    fn present_image(&self, owner: *mut rsf_fgvk, index: usize) -> Option<(u64, u32)> {
        let mut count = 0;
        let mut layout = PRESENT_LAYOUT;
        let value = if !owner.is_null() {
            if unsafe { rsf_fgvk_images(owner, &mut count, ptr::null_mut()) } != backend_result::OK
                || count == 0
                || count as usize > SWAPCHAIN_IMAGES
            {
                return None;
            }
            let mut images = [0u64; SWAPCHAIN_IMAGES];
            if unsafe { rsf_fgvk_images(owner, &mut count, images.as_mut_ptr()) }
                != backend_result::OK
                || unsafe { rsf_fgvk_image_layout(owner, &mut layout) } != backend_result::OK
                || index >= count as usize
            {
                return None;
            }
            *images.get(index)?
        } else {
            let proc = self.setup.graphics.device_proc?;
            let device = self.setup.graphics.device;
            let get = unsafe {
                std::mem::transmute::<Option<VkFunction>, Option<vk::PFN_vkGetSwapchainImagesKHR>>(
                    proc(device, c"vkGetSwapchainImagesKHR".as_ptr()),
                )
            }?;
            if unsafe { get(handle(device), self.chain, &mut count, ptr::null_mut()) }
                != vk::Result::SUCCESS
                || count == 0
                || count as usize > SWAPCHAIN_IMAGES
            {
                return None;
            }
            let mut images = [vk::Image::null(); SWAPCHAIN_IMAGES];
            if unsafe { get(handle(device), self.chain, &mut count, images.as_mut_ptr()) }
                != vk::Result::SUCCESS
                || index >= count as usize
            {
                return None;
            }
            images.get(index)?.as_raw()
        };
        Some((value, layout))
    }
}
