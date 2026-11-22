// SPDX-License-Identifier: MIT
use super::ffx_descriptors::abi::{
    PFN_vkVoidFunction, PfnFfxCreateContext, PfnFfxDestroyContext, PfnFfxDispatch, PfnFfxQuery,
    VkDevice, ffxContext,
};
use super::{
    DISPATCH, FfxDeviceEntries, FfxExports, Frame, Handle, INIT_FAILED, Image, Setup,
    lookup_device_proc, rsf_vk_function,
};
use crate::frame_capture_vk::{
    Owner as Capture, rsf_core_capture_vk_create, rsf_core_capture_vk_destroy,
    rsf_core_capture_vk_flush, rsf_core_capture_vk_initialize, rsf_core_capture_vk_provider,
    rsf_core_capture_vk_record, rsf_core_capture_vk_retire, rsf_core_capture_vk_wants,
};
use crate::sys::rsf_vkfg_image_copy as ImageCopy;
use crate::vulkan_fg_copy::{
    Barrier, COLOR_RANGE, rsf_core_vkfg_record_barrier, rsf_core_vkfg_record_copy,
};
use crate::vulkan_fg_resources::device::{
    Device, rsf_core_vkfg_bind_images, rsf_core_vkfg_device_destroy, rsf_core_vkfg_device_memory,
};
use crate::vulkan_sharpening::Host as Sharpen;
use crate::vulkan_sharpening::{
    rsf_sharpenvk_host_clear, rsf_sharpenvk_host_destroy, rsf_sharpenvk_host_retire,
    rsf_sharpenvk_host_run,
};
use ash::vk::{self, Handle as _};
use rsf_game_sys::rsf_frame_record;
use std::{ffi::c_char, ptr};

// The native exception guards borrow this typed SDK state from the Rust owner.
#[repr(C)]
#[derive(Default)]
struct Sdk {
    create: PfnFfxCreateContext,
    destroy: PfnFfxDestroyContext,
    query: PfnFfxQuery,
    dispatch: PfnFfxDispatch,
    effect: ffxContext,
}

const _: () = {
    assert!(size_of::<Sdk>() == 40 && align_of::<Sdk>() == 8);
    assert!(std::mem::offset_of!(Sdk, create) == 0);
    assert!(std::mem::offset_of!(Sdk, destroy) == 8);
    assert!(std::mem::offset_of!(Sdk, query) == 16);
    assert!(std::mem::offset_of!(Sdk, dispatch) == 24);
    assert!(std::mem::offset_of!(Sdk, effect) == 32);
};

#[derive(Default)]
struct Native {
    sdk: Sdk,
    graphics: Option<Graphics>,
}

impl Native {
    fn graphics(&self) -> &Graphics {
        self.graphics
            .as_ref()
            .expect("initialized Vulkan SR graphics")
    }

    fn graphics_mut(&mut self) -> &mut Graphics {
        self.graphics
            .as_mut()
            .expect("initialized Vulkan SR graphics")
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_srvk_native_allocate(_setup: &Setup) -> *mut Native {
    crate::allocate_handle(Native::default())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_native_close(owner: *mut Native) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_srvk_native_bind(owner: &mut Native, exports: &FfxExports) {
    let [create, destroy, query, dispatch] = *exports;
    // Admission resolved these four non-null exports by their exact SDK names.
    owner.sdk = unsafe {
        Sdk {
            create: std::mem::transmute::<rsf_vk_function, PfnFfxCreateContext>(create),
            destroy: std::mem::transmute::<rsf_vk_function, PfnFfxDestroyContext>(destroy),
            query: std::mem::transmute::<rsf_vk_function, PfnFfxQuery>(query),
            dispatch: std::mem::transmute::<rsf_vk_function, PfnFfxDispatch>(dispatch),
            effect: ptr::null_mut(),
        }
    };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_srvk_native_sdk(owner: &mut Native) -> *mut Sdk {
    &raw mut owner.sdk
}

pub(super) unsafe extern "C" fn sdk_device_proc(
    device: VkDevice,
    name: *const c_char,
) -> PFN_vkVoidFunction {
    let device = device.cast();
    let (proc, lookup) = DISPATCH
        .lock()
        .unwrap()
        .get(&(device as usize))
        .and_then(|entry| Some((entry.proc, entry.lookup?)))?;
    unsafe { lookup_device_proc(device, name, proc, lookup) }
}

struct Graphics {
    images: *mut Device,
    capture: *mut Capture,
    sharpen: Sharpen,
    memory_types: u32,
    barrier: vk::PFN_vkCmdPipelineBarrier,
    clear: vk::PFN_vkCmdClearColorImage,
    copy: vk::PFN_vkCmdCopyImage,
}

impl Drop for Graphics {
    fn drop(&mut self) {
        unsafe {
            rsf_sharpenvk_host_destroy(&mut self.sharpen);
            rsf_core_capture_vk_destroy(self.capture);
            rsf_core_vkfg_device_destroy(self.images);
        }
    }
}

fn create(
    setup: &Setup,
    entries: &FfxDeviceEntries,
    properties: rsf_vk_function,
) -> Option<Graphics> {
    // Admission resolves these slots by their exact Vulkan command names.
    macro_rules! procedure {
        ($value:expr, $ty:ident) => {
            unsafe { std::mem::transmute::<unsafe extern "C" fn(), vk::$ty>($value?) }
        };
    }
    let properties = procedure!(properties, PFN_vkGetPhysicalDeviceMemoryProperties);
    let [
        create,
        destroy,
        requirements,
        allocate,
        free,
        bind,
        barrier,
        clear,
        copy,
    ] = *entries;
    let mut graphics = Graphics {
        images: ptr::null_mut(),
        capture: ptr::null_mut(),
        sharpen: Sharpen::default(),
        memory_types: 0,
        barrier: procedure!(barrier, PFN_vkCmdPipelineBarrier),
        clear: procedure!(clear, PFN_vkCmdClearColorImage),
        copy: procedure!(copy, PFN_vkCmdCopyImage),
    };
    graphics.images = rsf_core_vkfg_bind_images(
        setup.device,
        &[create, destroy, requirements, allocate, free, bind],
    );
    let images = unsafe { graphics.images.as_mut() }?;
    let mut memory = vk::PhysicalDeviceMemoryProperties::default();
    unsafe {
        properties(
            vk::PhysicalDevice::from_raw(setup.physical_device as usize as u64),
            &mut memory,
        );
    }
    graphics.memory_types = memory.memory_type_count;
    rsf_core_vkfg_device_memory(images, &memory);
    graphics.capture = rsf_core_capture_vk_create(1);
    let ready = unsafe {
        rsf_core_capture_vk_initialize(graphics.capture, setup.device, setup.device_proc, &memory)
    };
    (ready != 0).then_some(graphics)
}

fn command_buffer(commands: Handle) -> vk::CommandBuffer {
    vk::CommandBuffer::from_raw(commands as usize as u64)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_srvk_graphics_initialize(
    owner: &mut Native,
    setup: &Setup,
    entries: &FfxDeviceEntries,
    properties: rsf_vk_function,
) -> i32 {
    let Some(graphics) = create(setup, entries, properties) else {
        return INIT_FAILED;
    };
    owner.graphics = Some(graphics);
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_srvk_graphics_images(owner: &Native) -> Handle {
    let graphics = owner.graphics();
    graphics.images.cast()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_srvk_graphics_memory_count(owner: &Native) -> u32 {
    let graphics = owner.graphics();
    graphics.memory_types
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_barrier(
    owner: &Native,
    commands: Handle,
    plan: &Barrier,
) {
    let graphics = owner.graphics();
    unsafe { rsf_core_vkfg_record_barrier(graphics.barrier, command_buffer(commands), plan) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_clear(
    owner: &Native,
    commands: Handle,
    image: u64,
    layout: u32,
) {
    let graphics = owner.graphics();
    unsafe {
        (graphics.clear)(
            command_buffer(commands),
            vk::Image::from_raw(image),
            vk::ImageLayout::from_raw(layout as i32),
            &vk::ClearColorValue { uint32: [0; 4] },
            1,
            &COLOR_RANGE,
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_copy(
    owner: &Native,
    commands: Handle,
    plan: &ImageCopy,
) {
    let graphics = owner.graphics();
    unsafe { rsf_core_vkfg_record_copy(graphics.copy, command_buffer(commands), plan) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_sharpen(
    owner: &mut Native,
    setup: &Setup,
    commands: Handle,
    frame: &Frame,
    output: u64,
) -> i32 {
    let graphics = owner.graphics_mut();
    i32::from(
        unsafe { rsf_sharpenvk_host_run(&mut graphics.sharpen, setup, commands, frame, output) }
            != 0,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_clear_sharpen(owner: &mut Native) {
    if let Some(graphics) = owner.graphics.as_mut() {
        unsafe { rsf_sharpenvk_host_clear(&mut graphics.sharpen) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_capture_provider(
    owner: &Native,
    version: u64,
    backend: u32,
) {
    let graphics = owner.graphics();
    unsafe { rsf_core_capture_vk_provider(graphics.capture, version, backend, 0) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_capture_record(
    owner: &Native,
    commands: Handle,
    image: &Image,
    record: &rsf_frame_record,
    role: *const c_char,
    reset: u32,
) {
    let graphics = owner.graphics();
    unsafe {
        rsf_core_capture_vk_record(
            graphics.capture,
            commands,
            image,
            record,
            role,
            reset,
            1,
            ptr::null_mut(),
            0,
        )
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_capture_wants(owner: &Native, frame: u64) -> i32 {
    let graphics = owner.graphics();
    unsafe { rsf_core_capture_vk_wants(graphics.capture, frame) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_retire(owner: &mut Native, commands: Handle) {
    let graphics = owner.graphics_mut();
    unsafe {
        rsf_sharpenvk_host_retire(&mut graphics.sharpen, commands);
        rsf_core_capture_vk_retire(graphics.capture, commands);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_flush(owner: &Native) {
    if let Some(graphics) = owner.graphics.as_ref() {
        unsafe { rsf_core_capture_vk_flush(graphics.capture, 0, ptr::null_mut()) };
    }
}

unsafe extern "C" {
    fn rsf_capture_sr_parameters(
        record: *const rsf_frame_record,
        backend: u32,
        x: f32,
        y: f32,
        exposure: f32,
        reset: u32,
    );
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_capture_parameters(
    frame: &Frame,
    backend: u32,
    reset: u32,
) {
    // Use the same FrameDump entry, including its once-only service binding.
    unsafe {
        rsf_capture_sr_parameters(
            frame.record,
            backend,
            frame.motion_scale_x,
            frame.motion_scale_y,
            frame.pre_exposure,
            reset,
        );
    }
}
