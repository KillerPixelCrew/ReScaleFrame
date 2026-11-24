// SPDX-License-Identifier: MIT
use super::*;
use crate::dx12_vulkan_platform::{adapter_luid, device_local_mask, entry};
use crate::frame_capture_vk::{
    Owner as Capture, rsf_core_capture_vk_complete_group, rsf_core_capture_vk_discard_group,
    rsf_core_capture_vk_flush, rsf_core_capture_vk_provider, rsf_core_capture_vk_record,
};
use crate::vulkan_fg_copy::{
    COLOR_RANGE, ImageCopy, rsf_core_vkfg_record_barrier as record_barrier,
    rsf_core_vkfg_record_copy as record_copy,
};
use ash::vk::{self, Handle as _};

macro_rules! device_dispatch {
    ($($field:ident: $ty:ident = $name:literal),+ $(,)?) => {
        struct Dispatch {
            $($field: vk::$ty,)+
            memory_properties: vk::PFN_vkGetPhysicalDeviceMemoryProperties,
            format_properties: vk::PFN_vkGetPhysicalDeviceFormatProperties,
        }

        impl Dispatch {
            fn load(graphics: &Graphics) -> Option<Self> {
                graphics.instance_proc?;
                // Each name is paired with its exact system-ABI signature.
                $(let $field = unsafe { entry::<vk::$ty>(graphics.device_proc, graphics.device, $name) }?;)+
                let memory = unsafe { entry(graphics.instance_proc, graphics.instance, c"vkGetPhysicalDeviceMemoryProperties") };
                let formats = unsafe { entry(graphics.instance_proc, graphics.instance, c"vkGetPhysicalDeviceFormatProperties") };
                Some(Self {
                    $($field,)+
                    memory_properties: memory?,
                    format_properties: formats?,
                })
            }
        }
    };
}

device_dispatch! {
    create_image: PFN_vkCreateImage = c"vkCreateImage",
    destroy_image: PFN_vkDestroyImage = c"vkDestroyImage",
    requirements: PFN_vkGetImageMemoryRequirements = c"vkGetImageMemoryRequirements",
    allocate: PFN_vkAllocateMemory = c"vkAllocateMemory",
    free_memory: PFN_vkFreeMemory = c"vkFreeMemory",
    bind: PFN_vkBindImageMemory = c"vkBindImageMemory",
    create_pool: PFN_vkCreateCommandPool = c"vkCreateCommandPool",
    destroy_pool: PFN_vkDestroyCommandPool = c"vkDestroyCommandPool",
    allocate_commands: PFN_vkAllocateCommandBuffers = c"vkAllocateCommandBuffers",
    reset_commands: PFN_vkResetCommandBuffer = c"vkResetCommandBuffer",
    begin: PFN_vkBeginCommandBuffer = c"vkBeginCommandBuffer",
    end: PFN_vkEndCommandBuffer = c"vkEndCommandBuffer",
    barrier: PFN_vkCmdPipelineBarrier = c"vkCmdPipelineBarrier",
    copy: PFN_vkCmdCopyImage = c"vkCmdCopyImage",
    blit: PFN_vkCmdBlitImage = c"vkCmdBlitImage",
    clear: PFN_vkCmdClearColorImage = c"vkCmdClearColorImage",
    wait: PFN_vkWaitSemaphores = c"vkWaitSemaphores",
    idle: PFN_vkDeviceWaitIdle = c"vkDeviceWaitIdle",
    create_fence: PFN_vkCreateFence = c"vkCreateFence",
    reset_fences: PFN_vkResetFences = c"vkResetFences",
    wait_fences: PFN_vkWaitForFences = c"vkWaitForFences",
    destroy_fence: PFN_vkDestroyFence = c"vkDestroyFence",
}

#[cfg(test)]
pub(super) fn dispatch_available(graphics: &Graphics) -> bool {
    Dispatch::load(graphics).is_some()
}

struct Platform {
    setup: Setup,
    dispatch: Option<Dispatch>,
    memory: vk::PhysicalDeviceMemoryProperties,
    capture: Capture,
}

impl Platform {
    fn device(&self) -> vk::Device {
        vk::Device::from_raw(self.setup.graphics.device as u64)
    }

    fn physical(&self) -> vk::PhysicalDevice {
        vk::PhysicalDevice::from_raw(self.setup.graphics.physical_device as u64)
    }

    fn submit(&self, submits: &[vk::SubmitInfo<'_>], fence: u64) -> i32 {
        // Queue submission stays on the host callback and preserves its locking contract.
        unsafe {
            (self.setup.graphics_submit.unwrap())(
                self.setup.queue_user,
                submits.len() as u32,
                if submits.is_empty() {
                    ptr::null()
                } else {
                    submits.as_ptr().cast()
                },
                fence,
            )
        }
    }
}

unsafe extern "C" fn platform_allocate(setup: *const Setup) -> Handle {
    allocate_handle(Platform {
        setup: unsafe { *setup },
        dispatch: None,
        memory: vk::PhysicalDeviceMemoryProperties::default(),
        capture: Capture::new_native(false),
    })
    .cast()
}

unsafe extern "C" fn platform_destroy(platform: Handle) {
    drop(unsafe { Box::from_raw(platform.cast::<Platform>()) });
}

unsafe fn raw_result<T: vk::Handle>(result: vk::Result, value: T, out: *mut u64) -> i32 {
    if result == vk::Result::SUCCESS {
        unsafe { out.write(value.as_raw()) };
    }
    result.as_raw()
}

fn command(list: Handle) -> vk::CommandBuffer {
    vk::CommandBuffer::from_raw(list as u64)
}

fn dispatch_for(platform: &Platform) -> &Dispatch {
    platform.dispatch.as_ref().unwrap()
}

unsafe extern "C" fn platform_resolve(platform: Handle, graphics: *const Graphics) -> i32 {
    let platform = unsafe { &mut *platform.cast::<Platform>() };
    platform.dispatch = Dispatch::load(unsafe { &*graphics });
    if platform.dispatch.is_some() {
        0
    } else {
        EXTENSION_NOT_PRESENT
    }
}

unsafe extern "C" fn format_features(platform: Handle, format: u32) -> u32 {
    let platform = unsafe { &*platform.cast::<Platform>() };
    let mut properties = vk::FormatProperties::default();
    unsafe {
        (dispatch_for(platform).format_properties)(
            platform.physical(),
            vk::Format::from_raw(format as i32),
            &mut properties,
        );
    }
    properties.optimal_tiling_features.as_raw()
}

unsafe extern "C" fn initialize_capture(platform: Handle, graphics: *const Graphics) -> i32 {
    let (platform, graphics) = unsafe { (&mut *platform.cast::<Platform>(), &*graphics) };
    let properties = dispatch_for(platform).memory_properties;
    unsafe { properties(platform.physical(), &mut platform.memory) };
    if platform
        .capture
        .initialize(graphics.device, graphics.device_proc, &platform.memory)
    {
        vk::Result::SUCCESS.as_raw()
    } else {
        vk::Result::ERROR_INITIALIZATION_FAILED.as_raw()
    }
}

unsafe extern "C" fn capture_provider(platform: Handle, version: u64, backend: u32) {
    let platform = unsafe { &*platform.cast::<Platform>() };
    unsafe { rsf_core_capture_vk_provider(&platform.capture, version, backend, 1) };
}

unsafe extern "C" fn capture_record(
    platform: Handle,
    commands: Handle,
    source: *const Image,
    record: *const Record,
    role: *const c_char,
    group: u64,
) {
    let platform = unsafe { &*platform.cast::<Platform>() };
    let reset = unsafe { (*record).flags } & rsf_game_sys::RSF_FRAME_FLAG_RESET;
    unsafe {
        rsf_core_capture_vk_record(
            &platform.capture,
            commands,
            source,
            record,
            role,
            reset,
            1,
            ptr::null_mut(),
            group,
        )
    };
}

unsafe extern "C" fn capture_complete(platform: Handle, group: u64) {
    let platform = unsafe { &*platform.cast::<Platform>() };
    unsafe { rsf_core_capture_vk_complete_group(&platform.capture, group) };
}

unsafe extern "C" fn capture_discard(platform: Handle, group: u64) {
    let platform = unsafe { &*platform.cast::<Platform>() };
    unsafe { rsf_core_capture_vk_discard_group(&platform.capture, group) };
}

unsafe extern "C" fn capture_flush(platform: Handle) -> i32 {
    let platform = unsafe { &*platform.cast::<Platform>() };
    if unsafe { rsf_core_capture_vk_flush(&platform.capture, 0, ptr::null_mut()) } != 0 {
        vk::Result::SUCCESS.as_raw()
    } else {
        vk::Result::ERROR_UNKNOWN.as_raw()
    }
}

static IMAGES: ImageOps = ImageOps {
    local_memory_types: Some(local_memory_types),
    create_image: Some(create_image),
    requirements: Some(image_requirements),
    allocate: Some(allocate_memory),
    bind: Some(bind_image),
    create_view: None,
    destroy_view: None,
    destroy_image: Some(destroy_image),
    free_memory: Some(free_memory),
    create_fence: None,
    destroy_fence: None,
    wait_fence: None,
    reset_fence: None,
    submit_fence: None,
};

unsafe extern "C" fn local_memory_types(platform: Handle) -> u32 {
    device_local_mask(unsafe { &(*platform.cast::<Platform>()).memory })
}

unsafe extern "C" fn create_image(
    platform: Handle,
    width: u32,
    height: u32,
    format: u32,
    usage: u32,
    output: *mut u64,
) -> i32 {
    let platform = unsafe { &*platform.cast::<Platform>() };
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(vk::Format::from_raw(format as i32))
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(vk::ImageUsageFlags::from_raw(usage));
    let mut image = vk::Image::null();
    let result = unsafe {
        (dispatch_for(platform).create_image)(platform.device(), &info, ptr::null(), &mut image)
    };
    unsafe { raw_result(result, image, output) }
}

unsafe extern "C" fn image_requirements(
    platform: Handle,
    image: u64,
    size: *mut u64,
    types: *mut u32,
) {
    let platform = unsafe { &*platform.cast::<Platform>() };
    let mut need = vk::MemoryRequirements::default();
    unsafe {
        (dispatch_for(platform).requirements)(
            platform.device(),
            vk::Image::from_raw(image),
            &mut need,
        );
        size.write(need.size);
        types.write(need.memory_type_bits);
    }
}

unsafe extern "C" fn allocate_memory(
    platform: Handle,
    size: u64,
    ty: u32,
    output: *mut u64,
) -> i32 {
    let platform = unsafe { &*platform.cast::<Platform>() };
    let info = vk::MemoryAllocateInfo::default()
        .allocation_size(size)
        .memory_type_index(ty);
    let mut memory = vk::DeviceMemory::null();
    let result = unsafe {
        (dispatch_for(platform).allocate)(platform.device(), &info, ptr::null(), &mut memory)
    };
    unsafe { raw_result(result, memory, output) }
}

unsafe extern "C" fn bind_image(platform: Handle, image: u64, memory: u64) -> i32 {
    let platform = unsafe { &*platform.cast::<Platform>() };
    unsafe {
        (dispatch_for(platform).bind)(
            platform.device(),
            vk::Image::from_raw(image),
            vk::DeviceMemory::from_raw(memory),
            0,
        )
        .as_raw()
    }
}

unsafe extern "C" fn destroy_image(platform: Handle, image: u64) {
    let platform = unsafe { &*platform.cast::<Platform>() };
    unsafe {
        (dispatch_for(platform).destroy_image)(
            platform.device(),
            vk::Image::from_raw(image),
            ptr::null(),
        )
    };
}

unsafe extern "C" fn free_memory(platform: Handle, memory: u64) {
    let platform = unsafe { &*platform.cast::<Platform>() };
    unsafe {
        (dispatch_for(platform).free_memory)(
            platform.device(),
            vk::DeviceMemory::from_raw(memory),
            ptr::null(),
        )
    };
}

unsafe extern "C" fn create_pool(platform: Handle, output: *mut u64) -> i32 {
    let platform = unsafe { &*platform.cast::<Platform>() };
    let info = vk::CommandPoolCreateInfo::default()
        .queue_family_index(platform.setup.queue_family)
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
    let mut pool = vk::CommandPool::null();
    let result = unsafe {
        (dispatch_for(platform).create_pool)(platform.device(), &info, ptr::null(), &mut pool)
    };
    unsafe { raw_result(result, pool, output) }
}

unsafe extern "C" fn destroy_pool(platform: Handle, pool: u64) {
    let platform = unsafe { &*platform.cast::<Platform>() };
    unsafe {
        (dispatch_for(platform).destroy_pool)(
            platform.device(),
            vk::CommandPool::from_raw(pool),
            ptr::null(),
        )
    };
}

unsafe extern "C" fn allocate_commands(platform: Handle, pool: u64, output: *mut Handle) -> i32 {
    let platform = unsafe { &*platform.cast::<Platform>() };
    let info = vk::CommandBufferAllocateInfo::default()
        .command_pool(vk::CommandPool::from_raw(pool))
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    let mut list = vk::CommandBuffer::null();
    let result =
        unsafe { (dispatch_for(platform).allocate_commands)(platform.device(), &info, &mut list) };
    if result == vk::Result::SUCCESS {
        unsafe { output.write(ptr::with_exposed_provenance_mut(list.as_raw() as usize)) };
    }
    result.as_raw()
}

unsafe extern "C" fn begin(platform: Handle, list: Handle) -> i32 {
    let dispatch = dispatch_for(unsafe { &*platform.cast::<Platform>() });
    let commands = command(list);
    let result =
        unsafe { (dispatch.reset_commands)(commands, vk::CommandBufferResetFlags::empty()) };
    if result != vk::Result::SUCCESS {
        return result.as_raw();
    }
    let info =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    unsafe { (dispatch.begin)(commands, &info).as_raw() }
}

unsafe extern "C" fn end(platform: Handle, list: Handle) -> i32 {
    let dispatch = dispatch_for(unsafe { &*platform.cast::<Platform>() });
    unsafe { (dispatch.end)(command(list)).as_raw() }
}

const LAYERS: vk::ImageSubresourceLayers = vk::ImageSubresourceLayers {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    mip_level: 0,
    base_array_layer: 0,
    layer_count: 1,
};

unsafe extern "C" fn barrier(platform: Handle, list: Handle, plan: *const Barrier) {
    let dispatch = dispatch_for(unsafe { &*platform.cast::<Platform>() });
    unsafe { record_barrier(dispatch.barrier, command(list), &*plan) };
}

unsafe extern "C" fn copy(platform: Handle, list: Handle, plan: *const Transfer) {
    let dispatch = dispatch_for(unsafe { &*platform.cast::<Platform>() });
    let plan = unsafe { &*plan };
    let copy = ImageCopy::transfer(plan.source, plan.target, plan.target_extent);
    unsafe { record_copy(dispatch.copy, command(list), &copy) };
}
unsafe extern "C" fn blit(platform: Handle, list: Handle, plan: *const Transfer) {
    let dispatch = dispatch_for(unsafe { &*platform.cast::<Platform>() });
    let plan = unsafe { &*plan };
    let offsets = |[width, height]: [u32; 2]| {
        [
            vk::Offset3D::default(),
            vk::Offset3D {
                x: width as i32,
                y: height as i32,
                z: 1,
            },
        ]
    };
    let region = vk::ImageBlit::default()
        .src_subresource(LAYERS)
        .dst_subresource(LAYERS)
        .src_offsets(offsets(plan.source_extent))
        .dst_offsets(offsets(plan.target_extent));
    unsafe {
        (dispatch.blit)(
            command(list),
            vk::Image::from_raw(plan.source),
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            vk::Image::from_raw(plan.target),
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            1,
            &region,
            vk::Filter::NEAREST,
        );
    }
}

unsafe extern "C" fn clear(platform: Handle, list: Handle, target: u64) {
    let dispatch = dispatch_for(unsafe { &*platform.cast::<Platform>() });
    let zero = vk::ClearColorValue { uint32: [0; 4] };
    unsafe {
        (dispatch.clear)(
            command(list),
            vk::Image::from_raw(target),
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &zero,
            1,
            &COLOR_RANGE,
        );
    }
}

unsafe extern "C" fn acquire_submit(platform: Handle, semaphore: u64, fence: u64) -> i32 {
    let platform = unsafe { &*platform.cast::<Platform>() };
    let signal = vk::Semaphore::from_raw(semaphore);
    let submit = vk::SubmitInfo::default().signal_semaphores(if semaphore == 0 {
        &[]
    } else {
        slice::from_ref(&signal)
    });
    platform.submit(&[submit], fence)
}

unsafe extern "C" fn present_submit(platform: Handle, list: Handle, plan: &PresentSubmit) -> i32 {
    let platform = unsafe { &*platform.cast::<Platform>() };
    let count = plan.wait_count as usize;
    let waits = plan.waits.map(vk::Semaphore::from_raw);
    let stages = plan.stages.map(vk::PipelineStageFlags::from_raw);
    let signal = vk::Semaphore::from_raw(plan.signal);
    let commands = command(list);
    let mut timeline = vk::TimelineSemaphoreSubmitInfo::default()
        .wait_semaphore_values(&plan.values[..count])
        .signal_semaphore_values(slice::from_ref(&plan.ready));
    let submit = vk::SubmitInfo::default()
        .wait_semaphores(&waits[..count])
        .wait_dst_stage_mask(&stages[..count])
        .signal_semaphores(slice::from_ref(&signal))
        .command_buffers(slice::from_ref(&commands))
        .push_next(&mut timeline);
    platform.submit(&[submit], 0)
}

unsafe extern "C" fn wait_timeline(
    platform: Handle,
    timeline: u64,
    value: u64,
    timeout: u64,
) -> i32 {
    let platform = unsafe { &*platform.cast::<Platform>() };
    let semaphore = vk::Semaphore::from_raw(timeline);
    let info = vk::SemaphoreWaitInfo::default()
        .semaphores(slice::from_ref(&semaphore))
        .values(slice::from_ref(&value));
    unsafe { (dispatch_for(platform).wait)(platform.device(), &info, timeout).as_raw() }
}

unsafe extern "C" fn create_fence(platform: Handle, output: *mut u64) -> i32 {
    let platform = unsafe { &*platform.cast::<Platform>() };
    let mut fence = vk::Fence::null();
    let result = unsafe {
        (dispatch_for(platform).create_fence)(
            platform.device(),
            &vk::FenceCreateInfo::default(),
            ptr::null(),
            &mut fence,
        )
    };
    unsafe { raw_result(result, fence, output) }
}

unsafe extern "C" fn mark_submit(platform: Handle, fence: u64) -> i32 {
    let platform = unsafe { &*platform.cast::<Platform>() };
    let reset = unsafe {
        (dispatch_for(platform).reset_fences)(platform.device(), 1, &vk::Fence::from_raw(fence))
    };
    if reset != vk::Result::SUCCESS {
        reset.as_raw()
    } else {
        platform.submit(&[], fence)
    }
}

unsafe extern "C" fn wait_fence(platform: Handle, fence: u64, timeout: u64) -> i32 {
    let platform = unsafe { &*platform.cast::<Platform>() };
    unsafe {
        (dispatch_for(platform).wait_fences)(
            platform.device(),
            1,
            &vk::Fence::from_raw(fence),
            vk::TRUE,
            timeout,
        )
        .as_raw()
    }
}

unsafe extern "C" fn destroy_fence(platform: Handle, fence: u64) {
    let platform = unsafe { &*platform.cast::<Platform>() };
    unsafe {
        (dispatch_for(platform).destroy_fence)(
            platform.device(),
            vk::Fence::from_raw(fence),
            ptr::null(),
        )
    };
}

unsafe extern "C" fn idle(platform: Handle) -> i32 {
    let platform = unsafe { &*platform.cast::<Platform>() };
    unsafe {
        (platform.setup.lock_queues.unwrap())(platform.setup.queue_user);
        let result = (dispatch_for(platform).idle)(platform.device());
        (platform.setup.unlock_queues.unwrap())(platform.setup.queue_user);
        result.as_raw()
    }
}

pub(super) unsafe extern "C" fn describe(native: *const c_void, output: *mut Swapchain) {
    let info = unsafe { &*native.cast::<vk::SwapchainCreateInfoKHR<'_>>() };
    let extra = unsafe { info.p_next.cast::<vk::BaseInStructure<'_>>().as_ref() };
    let (extension_count, extension_type) = extra.map_or((0, 0), |extra| {
        (
            1 + u32::from(!extra.p_next.is_null()),
            extra.s_type.as_raw() as u32,
        )
    });
    unsafe {
        output.write(Swapchain {
            width: info.image_extent.width,
            height: info.image_extent.height,
            format: info.image_format.as_raw() as u32,
            usage: info.image_usage.as_raw(),
            color_space: info.image_color_space.as_raw() as u32,
            layers: info.image_array_layers,
            sharing_mode: info.image_sharing_mode.as_raw() as u32,
            min_images: info.min_image_count,
            flags: info.flags.as_raw(),
            present_mode: info.present_mode.as_raw() as u32,
            extension_count,
            extension_type,
            old_chain: info.old_swapchain.as_raw(),
        })
    };
}

unsafe extern "C" fn read_present(
    native: *const c_void,
    count: u32,
    chain: &mut u64,
    index: &mut u32,
    waits: *mut u64,
) {
    let info = unsafe { &*native.cast::<vk::PresentInfoKHR<'_>>() };
    unsafe {
        *chain = (*info.p_swapchains).as_raw();
        *index = *info.p_image_indices;
        for i in 0..count as usize {
            waits
                .add(i)
                .write((*info.p_wait_semaphores.add(i)).as_raw());
        }
    }
}

pub(crate) unsafe fn rsf_dx12gvk_present_marshaled(
    call: PresentCallback,
    owner: Handle,
    native: *const c_void,
    source: *const Record,
    explicit: u32,
) -> i32 {
    let Some(info) = (unsafe { native.cast::<vk::PresentInfoKHR<'_>>().as_ref() }) else {
        return INIT_FAILED;
    };
    let input = PresentInput {
        native,
        swapchain_count: info.swapchain_count,
        wait_count: info.wait_semaphore_count,
        has_extension: u32::from(!info.p_next.is_null()),
        has_image: u32::from(!info.p_swapchains.is_null() && !info.p_image_indices.is_null()),
        has_waits: u32::from(!info.p_wait_semaphores.is_null()),
        has_result: u32::from(!info.p_results.is_null()),
    };
    let mut result = INCOMPLETE;
    let status = unsafe {
        marshal_present(
            call,
            owner,
            &input,
            read_present,
            source,
            explicit,
            &mut result,
        )
    };
    if !info.p_results.is_null() && result != INCOMPLETE {
        unsafe { info.p_results.write(vk::Result::from_raw(result)) };
    }
    status
}

/// The Vulkan graphics entries the FSR4 and XeSS owners bind in production.
pub(crate) fn graphics() -> &'static GraphicsOps {
    &GRAPHICS.0
}

struct StaticGraphics(GraphicsOps);

// The only raw pointer in this immutable table points to static ImageOps.
unsafe impl Sync for StaticGraphics {}

static GRAPHICS: StaticGraphics = StaticGraphics(GraphicsOps {
    adapter_luid,
    platform_allocate,
    platform_destroy,
    platform_resolve,
    format_features,
    initialize_capture,
    create_pool,
    destroy_pool,
    images: &IMAGES,
    allocate_commands,
    begin,
    end,
    barrier,
    copy,
    blit,
    clear,
    acquire_submit,
    present_submit,
    wait_timeline,
    create_fence,
    mark_submit,
    wait_fence,
    destroy_fence,
    idle,
    capture_provider,
    capture_record,
    capture_complete,
    capture_discard,
    capture_flush,
    describe,
});
