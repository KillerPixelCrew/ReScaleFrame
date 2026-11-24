// SPDX-License-Identifier: MIT
//! CPU-only XeSS Vulkan SR contract. The production Rust table and owner run
//! with inert SDK and Vulkan entry points substituted through the table copy.
use crate::sys::{
    RSF_SHARPENING_DEFAULT_MOTION_SCALE_LIMIT, RSF_SR_XESS, RSF_SRVK_ABI_VERSION, rsf_srvk_frame,
    rsf_srvk_image, rsf_srvk_setup, rsf_vk_function,
};
use crate::sys::{RSF_XESSVK_REQUIREMENTS_ABI_VERSION, rsf_xessvk_setup};
use crate::xess_sdk_abi::{
    XESS_QUALITY_SETTING_QUALITY, xess_2d_t, xess_quality_settings_t, xess_version_t,
    xess_vk_execute_params_t, xess_vk_init_params_t,
};
use ash::vk::{self, Handle as _};
use rsf_game_sys::{RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record};
use std::ffi::{CStr, c_char, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

type Handle = *mut c_void;

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const MISSING_ENTRY_POINT: i32 = -5;
const NOT_READY: i32 = -8;
const XESS_SUCCESS: i32 = 0;
const XESS_AUTOEXPOSURE: u32 = 1 << 8;
const CONTEXT: Handle = 0x40 as Handle;
// sharpening_settings.h
const SHARPENING_ABI_VERSION: u32 = 1;
const SHARPENING_OFF: u32 = 0;

// Slot indices of the private XessOps record (31 pointers). The loader pair
// occupies slots 0 and 1.
const SLOT_LOAD: usize = 0;
const SLOT_UNLOAD: usize = 1;
const SLOT_PIN: usize = 3;
const SLOT_LOOKUP: usize = 30;
type Ops = [usize; 31];

unsafe extern "C" {
    static rsf_xessvk_native_ops: Ops;
    fn rsf_core_xessvk_initialize(ops: *const Ops, setup: *const rsf_xessvk_setup) -> i32;
    fn rsf_core_xessvk_shutdown(ops: *const Ops) -> i32;
    fn rsf_srvk_create(setup: *const rsf_srvk_setup, owner: *mut Handle) -> i32;
    fn rsf_srvk_plan(owner: Handle, width: *mut u32, height: *mut u32) -> i32;
    fn rsf_srvk_evaluate(owner: Handle, command: Handle, frame: *const rsf_srvk_frame) -> i32;
    fn rsf_srvk_retire_commands(owner: Handle, command: Handle);
    fn rsf_srvk_destroy(owner: Handle) -> i32;
}

macro_rules! counters {
    ($($name:ident),*) => { $(static $name: AtomicU32 = AtomicU32::new(0);)* };
}
counters!(
    EXECUTED,
    COPIED,
    IMAGES,
    DESTROYED_IMAGES,
    VIEWS,
    DESTROYED_VIEWS,
    FREED,
    CONTEXTS,
    DESTROYED_CONTEXTS,
    SDK_LOOKUPS,
    SDK_PINS,
    EXPECTED_WIDTH,
    EXPECTED_HEIGHT,
    EXPECTED_RESET
);
static OMIT_CREATE: AtomicBool = AtomicBool::new(true);
static FAILURE: std::sync::Mutex<Option<&'static str>> = std::sync::Mutex::new(None);

// Records the first failed check inside SDK or Vulkan callbacks, which cannot unwind.
fn check(condition: bool, message: &'static str) {
    if !condition {
        let mut failure = FAILURE.lock().unwrap_or_else(|poison| poison.into_inner());
        failure.get_or_insert(message);
    }
}

fn count(counter: &AtomicU32) -> u32 {
    counter.load(Relaxed)
}

unsafe extern "C" fn create(_: Handle, _: Handle, device: Handle, out: *mut Handle) -> i32 {
    check(device == 1 as Handle, "context device");
    unsafe { *out = CONTEXT };
    CONTEXTS.fetch_add(1, Relaxed);
    XESS_SUCCESS
}

unsafe extern "C" fn initialize(_: Handle, init: *const xess_vk_init_params_t) -> i32 {
    let init = unsafe { &*init };
    check(
        init.outputResolution.x == 1920 && init.outputResolution.y == 1080,
        "init output resolution",
    );
    check(
        init.qualitySetting == XESS_QUALITY_SETTING_QUALITY && init.initFlags == XESS_AUTOEXPOSURE,
        "init quality and flags",
    );
    XESS_SUCCESS
}

unsafe extern "C" fn resolution(
    _: Handle,
    output: *const xess_2d_t,
    quality: xess_quality_settings_t,
    recommended: *mut xess_2d_t,
    minimum: *mut xess_2d_t,
    maximum: *mut xess_2d_t,
) -> i32 {
    let output = unsafe { &*output };
    check(
        output.x == 1920 && output.y == 1080 && quality == XESS_QUALITY_SETTING_QUALITY,
        "optimal resolution request",
    );
    unsafe {
        *recommended = xess_2d_t { x: 1130, y: 636 };
        *minimum = xess_2d_t { x: 1024, y: 576 };
        *maximum = xess_2d_t { x: 1280, y: 720 };
    }
    XESS_SUCCESS
}

unsafe extern "C" fn velocity(_: Handle, x: f32, y: f32) -> i32 {
    check(x == 1.0 && y == 1.0, "velocity scale");
    XESS_SUCCESS
}

unsafe extern "C" fn execute(_: Handle, _: Handle, params: *const xess_vk_execute_params_t) -> i32 {
    let p = unsafe { &*params };
    check(
        p.inputWidth == count(&EXPECTED_WIDTH)
            && p.inputHeight == count(&EXPECTED_HEIGHT)
            && p.resetHistory == count(&EXPECTED_RESET),
        "execute input extent and reset",
    );
    // Private allocations cover the SDK maximum. Active input is a genuine
    // subrectangle, not a fabricated VkImage allocation extent.
    check(
        p.colorTexture.width == 1280 && p.colorTexture.height == 720,
        "color allocation extent",
    );
    check(
        p.depthTexture.width == 1280 && p.velocityTexture.height == 720,
        "guide allocation extent",
    );
    check(
        p.outputTexture.width == 1920 && p.outputTexture.height == 1080,
        "output allocation extent",
    );
    check(
        !p.colorTexture.imageView.is_null()
            && !p.outputTexture.imageView.is_null()
            && p.exposureScale == 1.0,
        "views and exposure",
    );
    for texture in [
        &p.colorTexture,
        &p.depthTexture,
        &p.velocityTexture,
        &p.outputTexture,
    ] {
        let range = &texture.subresourceRange;
        check(
            range.aspectMask == vk::ImageAspectFlags::COLOR.as_raw()
                && range.baseMipLevel == 0
                && range.levelCount == 1
                && range.baseArrayLayer == 0
                && range.layerCount == 1,
            "colour-aspect single subresource",
        );
    }
    check(
        p.exposureScaleTexture.image.is_null() && p.responsivePixelMaskTexture.image.is_null(),
        "null optional textures",
    );
    let zero = |base: &xess_2d_t| base.x == 0 && base.y == 0;
    check(
        zero(&p.inputColorBase)
            && zero(&p.inputDepthBase)
            && zero(&p.inputMotionVectorBase)
            && zero(&p.outputColorBase),
        "zero texture bases",
    );
    EXECUTED.fetch_add(1, Relaxed);
    XESS_SUCCESS
}

unsafe extern "C" fn destroy(context: Handle) -> i32 {
    check(context == CONTEXT, "destroyed context");
    DESTROYED_CONTEXTS.fetch_add(1, Relaxed);
    XESS_SUCCESS
}

unsafe extern "C" fn version(value: *mut xess_version_t) -> i32 {
    unsafe {
        *value = xess_version_t {
            major: 2,
            minor: 1,
            patch: 3,
            reserved: 4,
        }
    };
    XESS_SUCCESS
}

unsafe extern "C" fn instance_extensions(_: *mut u32, _: Handle, _: *mut u32) -> i32 {
    XESS_SUCCESS
}

unsafe extern "C" fn device_extensions(_: Handle, _: Handle, _: *mut u32, _: Handle) -> i32 {
    XESS_SUCCESS
}

unsafe extern "C" fn device_features(_: Handle, _: Handle, _: *mut Handle) -> i32 {
    XESS_SUCCESS
}

const EXPORTS: [&CStr; 10] = [
    c"xessVKGetRequiredInstanceExtensions",
    c"xessVKGetRequiredDeviceExtensions",
    c"xessVKGetRequiredDeviceFeatures",
    c"xessVKCreateContext",
    c"xessVKInit",
    c"xessVKExecute",
    c"xessDestroyContext",
    c"xessGetOptimalInputResolution",
    c"xessSetVelocityScale",
    c"xessGetVersion",
];

fn export_address(index: usize) -> Handle {
    let address: [*const (); 10] = [
        instance_extensions as *const (),
        device_extensions as *const (),
        device_features as *const (),
        create as *const (),
        initialize as *const (),
        execute as *const (),
        destroy as *const (),
        resolution as *const (),
        velocity as *const (),
        version as *const (),
    ];
    address[index] as Handle
}

fn sdk_module() -> Handle {
    (&raw const SDK_LOOKUPS).cast_mut().cast()
}

unsafe extern "C" fn sdk_load(_: *const u16) -> Handle {
    sdk_module()
}

unsafe extern "C" fn sdk_unload(_: Handle) {}

unsafe extern "C" fn sdk_pin(_: Handle) -> u32 {
    SDK_PINS.fetch_add(1, Relaxed);
    1
}

unsafe extern "C" fn sdk_lookup(module: Handle, name: *const c_char) -> Handle {
    let index = SDK_LOOKUPS.fetch_add(1, Relaxed) as usize;
    check(
        module == sdk_module() && index < EXPORTS.len(),
        "lookup module and count",
    );
    let Some(expected) = EXPORTS.get(index) else {
        return null_mut();
    };
    check(
        unsafe { CStr::from_ptr(name) } == *expected,
        "lookup export order",
    );
    if OMIT_CREATE.load(Relaxed) && index + 1 == 4 {
        null_mut()
    } else {
        export_address(index)
    }
}

unsafe extern "system" fn create_image(
    _: vk::Device,
    info: *const vk::ImageCreateInfo<'_>,
    _: *const c_void,
    out: *mut vk::Image,
) -> vk::Result {
    let info = unsafe { &*info };
    let (width, height, usage) = (info.extent.width, info.extent.height, info.usage.as_raw());
    check(
        (width == 1280 && height == 720 && usage == 6)
            || (width == 1920 && height == 1080 && usage == 15),
        "private image extent and usage",
    );
    let index = IMAGES.fetch_add(1, Relaxed) + 1;
    unsafe { *out = vk::Image::from_raw(0x100 + u64::from(index)) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn destroy_image(_: vk::Device, _: vk::Image, _: *const c_void) {
    DESTROYED_IMAGES.fetch_add(1, Relaxed);
}

unsafe extern "system" fn requirements(
    _: vk::Device,
    _: vk::Image,
    out: *mut vk::MemoryRequirements,
) {
    unsafe {
        *out = vk::MemoryRequirements {
            size: 1,
            alignment: 0,
            memory_type_bits: 1,
        }
    };
}

unsafe extern "system" fn allocate_memory(
    _: vk::Device,
    info: *const vk::MemoryAllocateInfo<'_>,
    _: *const c_void,
    out: *mut vk::DeviceMemory,
) -> vk::Result {
    check(
        unsafe { (*info).memory_type_index } == 0,
        "memory type index",
    );
    unsafe { *out = vk::DeviceMemory::from_raw(0x200 + u64::from(count(&IMAGES))) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn free_memory(_: vk::Device, _: vk::DeviceMemory, _: *const c_void) {
    FREED.fetch_add(1, Relaxed);
}

unsafe extern "system" fn bind(
    _: vk::Device,
    _: vk::Image,
    _: vk::DeviceMemory,
    _: vk::DeviceSize,
) -> vk::Result {
    vk::Result::SUCCESS
}

unsafe extern "system" fn create_view(
    _: vk::Device,
    _: *const vk::ImageViewCreateInfo<'_>,
    _: *const c_void,
    out: *mut vk::ImageView,
) -> vk::Result {
    let index = VIEWS.fetch_add(1, Relaxed) + 1;
    unsafe { *out = vk::ImageView::from_raw(0x300 + u64::from(index)) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn destroy_view(_: vk::Device, _: vk::ImageView, _: *const c_void) {
    DESTROYED_VIEWS.fetch_add(1, Relaxed);
}

#[allow(clippy::too_many_arguments)]
unsafe extern "system" fn barrier(
    _: vk::CommandBuffer,
    _: vk::PipelineStageFlags,
    _: vk::PipelineStageFlags,
    _: vk::DependencyFlags,
    _: u32,
    _: *const c_void,
    _: u32,
    _: *const c_void,
    _: u32,
    _: *const c_void,
) {
}

unsafe extern "system" fn copy(
    _: vk::CommandBuffer,
    _: vk::Image,
    _: vk::ImageLayout,
    _: vk::Image,
    _: vk::ImageLayout,
    _: u32,
    region: *const vk::ImageCopy,
) {
    let extent = unsafe { (*region).extent };
    check(
        (extent.width == count(&EXPECTED_WIDTH) && extent.height == count(&EXPECTED_HEIGHT))
            || (extent.width == 1920 && extent.height == 1080),
        "copy extent",
    );
    COPIED.fetch_add(1, Relaxed);
}

unsafe extern "system" fn memory_properties(
    _: vk::PhysicalDevice,
    out: *mut vk::PhysicalDeviceMemoryProperties,
) {
    let mut properties = vk::PhysicalDeviceMemoryProperties {
        memory_type_count: 1,
        ..Default::default()
    };
    properties.memory_types[0].property_flags = vk::MemoryPropertyFlags::DEVICE_LOCAL;
    unsafe { *out = properties };
}

fn entry(function: *const ()) -> rsf_vk_function {
    // Windows x64 uses one calling convention for system and C procedure pointers.
    unsafe { std::mem::transmute::<*const (), rsf_vk_function>(function) }
}

unsafe extern "C" fn device_proc(_: Handle, name: *const c_char) -> rsf_vk_function {
    let table: [(&CStr, *const ()); 10] = [
        (c"vkCreateImage", create_image as *const ()),
        (c"vkDestroyImage", destroy_image as *const ()),
        (c"vkGetImageMemoryRequirements", requirements as *const ()),
        (c"vkAllocateMemory", allocate_memory as *const ()),
        (c"vkFreeMemory", free_memory as *const ()),
        (c"vkBindImageMemory", bind as *const ()),
        (c"vkCreateImageView", create_view as *const ()),
        (c"vkDestroyImageView", destroy_view as *const ()),
        (c"vkCmdPipelineBarrier", barrier as *const ()),
        (c"vkCmdCopyImage", copy as *const ()),
    ];
    let name = unsafe { CStr::from_ptr(name) };
    // Capture stays disabled without its buffer entry points.
    table
        .iter()
        .find(|(entry_name, _)| *entry_name == name)
        .and_then(|(_, function)| entry(*function))
}

unsafe extern "C" fn instance_proc(_: Handle, name: *const c_char) -> rsf_vk_function {
    if unsafe { CStr::from_ptr(name) } == c"vkGetPhysicalDeviceMemoryProperties" {
        entry(memory_properties as *const ())
    } else {
        None
    }
}

fn require(condition: bool, message: &'static str) -> Result<(), &'static str> {
    let recorded = *FAILURE.lock().unwrap_or_else(|poison| poison.into_inner());
    match recorded {
        Some(failure) => Err(failure),
        None if condition => Ok(()),
        None => Err(message),
    }
}

fn image(
    handle: u64,
    format: vk::Format,
    width: u32,
    height: u32,
    layout: vk::ImageLayout,
) -> rsf_srvk_image {
    rsf_srvk_image {
        device: 1 as Handle,
        image: handle,
        format: format.as_raw() as u32,
        width,
        height,
        layout: layout.as_raw() as u32,
        copy_source: 1,
    }
}

fn run() -> Result<(), &'static str> {
    // The fixture owns a 'static copy so the session may retain it.
    let ops: &'static mut Ops = Box::leak(Box::new(unsafe { rsf_xessvk_native_ops }));
    ops[SLOT_LOAD] = sdk_load as *const () as usize;
    ops[SLOT_UNLOAD] = sdk_unload as *const () as usize;
    ops[SLOT_LOOKUP] = sdk_lookup as *const () as usize;
    ops[SLOT_PIN] = sdk_pin as *const () as usize;
    let session = rsf_xessvk_setup {
        struct_size: size_of::<rsf_xessvk_setup>() as u32,
        abi_version: RSF_XESSVK_REQUIREMENTS_ABI_VERSION,
        runtime_directory_utf8: c"C:\\fake".as_ptr(),
    };
    require(
        unsafe { rsf_core_xessvk_initialize(ops, &session) } == MISSING_ENTRY_POINT,
        "missing create export refuses the session",
    )?;
    require(
        count(&SDK_LOOKUPS) == 4 && count(&SDK_PINS) == 0,
        "lookup stops at the missing export",
    )?;
    OMIT_CREATE.store(false, Relaxed);
    SDK_LOOKUPS.store(0, Relaxed);
    require(
        unsafe { rsf_core_xessvk_initialize(ops, &session) } == OK,
        "complete export table opens the session",
    )?;
    require(
        count(&SDK_LOOKUPS) == 10 && count(&SDK_PINS) == 1,
        "all exports resolved and the module pinned",
    )?;

    let device = 1 as Handle;
    let setup = rsf_srvk_setup {
        struct_size: size_of::<rsf_srvk_setup>() as u32,
        abi_version: RSF_SRVK_ABI_VERSION,
        instance: device,
        physical_device: device,
        device,
        instance_proc: Some(instance_proc),
        device_proc: Some(device_proc),
        backend: RSF_SR_XESS,
        quality: 1,
        output_width: 1920,
        output_height: 1080,
        depth_inverted: 0,
        depth_infinite: 0,
        motion_jittered: 0,
        runtime_directory_utf8: c"C:\\fake".as_ptr(),
    };
    let mut owner = null_mut();
    require(
        unsafe { rsf_srvk_create(&setup, &mut owner) } == OK && !owner.is_null(),
        "create the SR owner",
    )?;
    require(
        count(&CONTEXTS) == 1 && count(&IMAGES) == 4 && count(&VIEWS) == 4,
        "context and private images",
    )?;
    require(
        unsafe { rsf_core_xessvk_shutdown(ops) } == NOT_READY,
        "live context refuses shutdown",
    )?;

    let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
    record.struct_size = size_of::<rsf_frame_record>() as u32;
    record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.camera.struct_size = size_of_val(&record.camera) as u32;
    record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    record.session_id = 1;
    record.view_id = 1;
    record.resource_generation = 1;
    record.frame_time_ms = 16.0;
    record.camera.near_plane = 0.1;
    record.camera.far_plane = 100.0;
    record.camera.vertical_fov_radians = 1.0;
    record.output_width = 1920;
    record.camera.output_width = 1920;
    record.output_height = 1080;
    record.camera.output_height = 1080;
    let mut input: rsf_srvk_frame = unsafe { std::mem::zeroed() };
    input.struct_size = size_of::<rsf_srvk_frame>() as u32;
    input.abi_version = RSF_SRVK_ABI_VERSION;
    input.pre_exposure = 1.0;
    input.motion_scale_x = 1.0;
    input.motion_scale_y = 1.0;
    input.sharpening.struct_size = size_of_val(&input.sharpening) as u32;
    input.sharpening.abi_version = SHARPENING_ABI_VERSION;
    input.sharpening.mode = SHARPENING_OFF;
    // The owner admits only a valid record (sharpening_policy::valid); the motion scale limit is the
    // one field whose floor is above zero.
    input.sharpening.motion_scale_limit = RSF_SHARPENING_DEFAULT_MOTION_SCALE_LIMIT as f32;

    let read = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
    let dimensions = |record: &mut rsf_frame_record, input: &mut rsf_srvk_frame, w: u32, h: u32| {
        record.render_width = w;
        record.camera.render_width = w;
        record.render_height = h;
        record.camera.render_height = h;
        input.color = image(20, vk::Format::R16G16B16A16_SFLOAT, w, h, read);
        input.depth = image(21, vk::Format::R32_SFLOAT, w, h, read);
        input.motion = image(22, vk::Format::R16G16_SFLOAT, w, h, read);
        input.output = image(
            23,
            vk::Format::R16G16B16A16_SFLOAT,
            1920,
            1080,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        );
        EXPECTED_WIDTH.store(w, Relaxed);
        EXPECTED_HEIGHT.store(h, Relaxed);
    };
    let commands = 2 as Handle;
    let evaluate = |record: &rsf_frame_record, input: &mut rsf_srvk_frame| {
        input.record = record;
        unsafe { rsf_srvk_evaluate(owner, commands, input) }
    };

    let (mut width, mut height) = (0, 0);
    require(
        unsafe { rsf_srvk_plan(owner, &mut width, &mut height) } == OK
            && width == 1130
            && height == 636,
        "recommended render size",
    )?;
    dimensions(&mut record, &mut input, 1128, 632);
    record.frame_id = 1;
    EXPECTED_RESET.store(1, Relaxed);
    require(
        evaluate(&record, &mut input) == OK && count(&EXECUTED) == 1 && count(&COPIED) == 4,
        "first frame resets history",
    )?;
    record.frame_id = 2;
    EXPECTED_RESET.store(0, Relaxed);
    require(
        evaluate(&record, &mut input) == OK && count(&EXECUTED) == 2,
        "consecutive frame keeps history",
    )?;
    dimensions(&mut record, &mut input, 1280, 720);
    record.frame_id = 3;
    EXPECTED_RESET.store(1, Relaxed);
    require(
        evaluate(&record, &mut input) == OK && count(&EXECUTED) == 3,
        "input size change resets history",
    )?;

    let reject = |record: &rsf_frame_record, input: &mut rsf_srvk_frame| {
        let before = count(&COPIED);
        require(
            evaluate(record, input) == INVALID_ARGUMENT
                && count(&COPIED) == before
                && count(&EXECUTED) == 3,
            "out-of-range input is refused before recording",
        )
    };
    for (w, h) in [(1023, 632), (1128, 575), (1281, 632), (1128, 721), (0, 632)] {
        dimensions(&mut record, &mut input, w, h);
        reject(&record, &mut input)?;
    }
    dimensions(&mut record, &mut input, 1128, 632);
    input.depth.width -= 1;
    reject(&record, &mut input)?;
    dimensions(&mut record, &mut input, 1128, 632);
    input.motion.height -= 1;
    reject(&record, &mut input)?;
    dimensions(&mut record, &mut input, 1128, 632);
    record.camera.render_width -= 1;
    reject(&record, &mut input)?;
    dimensions(&mut record, &mut input, 1128, 632);
    record.camera.output_height -= 1;
    reject(&record, &mut input)?;

    unsafe { rsf_srvk_retire_commands(owner, commands) };
    require(
        unsafe { rsf_srvk_destroy(owner) } == OK,
        "destroy the SR owner",
    )?;
    require(
        count(&DESTROYED_CONTEXTS) == 1
            && count(&DESTROYED_IMAGES) == 4
            && count(&DESTROYED_VIEWS) == 4
            && count(&FREED) == 4,
        "context and private images retired",
    )?;
    require(
        unsafe { rsf_core_xessvk_shutdown(ops) } == OK,
        "session shuts down after the context",
    )?;
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_xess_input_range() -> i32 {
    match run() {
        Ok(()) => {
            println!("PASS: XeSS Vulkan input range, cache sizing, reset and teardown");
            0
        }
        Err(failure) => {
            eprintln!("XeSS input range contract failed: {failure}");
            1
        }
    }
}
