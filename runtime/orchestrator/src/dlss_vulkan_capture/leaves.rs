// SPDX-License-Identifier: MIT
//! Typed Vulkan, WSI, Streamline registration and file leaves of the present
//! capture owner, formerly native_fg_dlss_vulkan_capture.cpp. The owner in
//! dlss_vulkan_capture.rs decides admission, command and fence lifetime,
//! submission order and retirement.
use super::{Callback, Handle, Packet, Platform};
use crate::frame_capture_vk::{
    Owner as Capture, rsf_core_capture_vk_complete_group, rsf_core_capture_vk_create,
    rsf_core_capture_vk_destroy, rsf_core_capture_vk_enabled, rsf_core_capture_vk_flush,
    rsf_core_capture_vk_initialize, rsf_core_capture_vk_pending, rsf_core_capture_vk_provider,
    rsf_core_capture_vk_record, rsf_core_capture_vk_wants,
};
use crate::sys::rsf_dlss_capture_target as Target;
use crate::sys::rsf_fgvk_setup as Setup;
use crate::sys::rsf_srvk_image as Image;
use crate::vulkan_capture_commands::Commands;
use ash::vk::{self, Handle as _};
use rsf_game_sys::backend_result::{
    INIT_FAILED, INVALID_ARGUMENT, MISSING_ENTRY_POINT, NOT_READY, OK,
};
use rsf_game_sys::{RSF_FRAME_FLAG_RESET, rsf_frame_record as Record};
use rsf_streamline_vk::sys::{
    RSF_SL_PRESENT_CAPTURE_ABI_VERSION, rsf_sl_present_capture_registration as Registration,
};
use std::ffi::{CStr, OsString, c_char, c_void};
use std::fs::OpenOptions;
use std::io::Write as _;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::PathBuf;
use std::ptr;
use windows_sys::Win32::System::Environment::GetEnvironmentVariableW;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

const SHARED_SUBMISSION: u32 = 0;

/// `rsfSlSetPresentCapture` of the corrected common plugin.
pub(crate) type Register = unsafe extern "C" fn(*const Registration) -> i32;
type WsiCallback = unsafe extern "C" fn(*mut c_void) -> i32;
type WsiCall = unsafe extern "C" fn(WsiCallback, *mut c_void) -> i32;

const _: () = {
    assert!(size_of::<vk::Image>() == size_of::<u64>());
    assert!(vk::ImageUsageFlags::TRANSFER_SRC.as_raw() == 1);
    assert!(vk::Format::R8G8B8A8_UNORM.as_raw() == 37 && vk::Format::B8G8R8A8_UNORM.as_raw() == 44);
    assert!(vk::ImageLayout::PRESENT_SRC_KHR.as_raw() == 1_000_001_002);
    assert!(vk::Result::ERROR_FORMAT_NOT_SUPPORTED.as_raw() == -11);
};

/// The capture leaves' native handle.
pub(crate) struct Native {
    setup: Setup,
    register: Register,
    capture: *mut Capture,
    directory: PathBuf,
    commands: Option<Commands>,
}

impl Native {
    /// A capture native without file output, with the given registration
    /// entry. The capture owner is created but not initialized.
    pub(crate) fn new(setup: &Setup, register: Register) -> Box<Self> {
        Box::new(Self {
            setup: *setup,
            register,
            capture: rsf_core_capture_vk_create(SHARED_SUBMISSION),
            directory: PathBuf::new(),
            commands: None,
        })
    }

    fn load_commands(&mut self) -> bool {
        self.commands = Commands::load(&self.setup.graphics);
        self.commands.is_some()
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        unsafe { rsf_core_capture_vk_destroy(self.capture) };
    }
}

fn native<'a>(native: Handle) -> &'a Native {
    unsafe { &*native.cast::<Native>() }
}

fn commands<'a>(handle: Handle) -> Option<&'a Commands> {
    native(handle).commands.as_ref()
}

fn present_info(packet: &Packet) -> &vk::PresentInfoKHR<'_> {
    unsafe { &*packet.present_info.cast() }
}

fn environment(name: &CStr) -> Option<OsString> {
    let name: Vec<u16> = name.to_str().ok()?.encode_utf16().chain([0]).collect();
    let mut value = vec![0u16; 32768];
    let length = unsafe { GetEnvironmentVariableW(name.as_ptr(), value.as_mut_ptr(), 32768) };
    (length != 0 && length < 32768).then(|| OsString::from_wide(&value[..length as usize]))
}

unsafe extern "C" fn open(setup: *const Setup, out: *mut Handle) -> i32 {
    unsafe { out.write(ptr::null_mut()) };
    let setup = unsafe { &*setup };
    let graphics = &setup.graphics;
    let capture = rsf_core_capture_vk_create(SHARED_SUBMISSION);
    let enabled = unsafe { rsf_core_capture_vk_enabled(capture) } != 0;
    unsafe { rsf_core_capture_vk_destroy(capture) };
    if !enabled {
        return OK;
    }
    let Some(directory) = environment(c"RSF_VULKAN_FRAME_CAPTURE_DIR") else {
        return INVALID_ARGUMENT;
    };
    if graphics.runtime_directory_utf8.is_null() {
        return INVALID_ARGUMENT;
    }
    let runtime = unsafe { CStr::from_ptr(graphics.runtime_directory_utf8) };
    let Ok(runtime) = runtime.to_str() else {
        return INVALID_ARGUMENT;
    };
    let common: Vec<u16> = PathBuf::from(runtime)
        .join("sl.common.dll")
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect();
    let module = unsafe { GetModuleHandleW(common.as_ptr()) };
    if module.is_null() {
        return NOT_READY;
    }
    let Some(register) =
        (unsafe { GetProcAddress(module, c"rsfSlSetPresentCapture".as_ptr().cast()) })
    else {
        return MISSING_ENTRY_POINT;
    };
    let register: Register = unsafe { std::mem::transmute(register) };
    let mut next = Native::new(setup, register);
    next.directory = PathBuf::from(directory);
    if !next.load_commands() {
        return MISSING_ENTRY_POINT;
    }
    let Some(instance_proc) = graphics.instance_proc else {
        return MISSING_ENTRY_POINT;
    };
    let name = c"vkGetPhysicalDeviceMemoryProperties";
    let Some(properties) = (unsafe { instance_proc(graphics.instance, name.as_ptr()) }) else {
        return MISSING_ENTRY_POINT;
    };
    let properties: vk::PFN_vkGetPhysicalDeviceMemoryProperties =
        unsafe { std::mem::transmute(properties) };
    let mut memory = vk::PhysicalDeviceMemoryProperties::default();
    unsafe {
        properties(
            vk::PhysicalDevice::from_raw(graphics.physical_device as u64),
            &mut memory,
        )
    };
    unsafe { rsf_core_capture_vk_provider(next.capture, 0, 1, 1) };
    if unsafe {
        rsf_core_capture_vk_initialize(next.capture, graphics.device, graphics.device_proc, &memory)
    } == 0
    {
        return INIT_FAILED;
    }
    unsafe { out.write(Box::into_raw(next).cast()) };
    OK
}

unsafe extern "C" fn close(handle: Handle) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle.cast::<Native>()) });
    }
}

unsafe extern "C" fn register(handle: Handle, callback: Option<Callback>, user: Handle) -> i32 {
    let native = native(handle);
    let registration = Registration {
        struct_size: size_of::<Registration>() as u32,
        abi_version: RSF_SL_PRESENT_CAPTURE_ABI_VERSION,
        device: native.setup.graphics.device,
        callback,
        user,
    };
    unsafe { (native.register)(&registration) }
}

fn result<T>(value: Result<T, vk::Result>, out: &mut T) -> i32 {
    match value {
        Ok(value) => {
            *out = value;
            OK
        }
        Err(error) => error.as_raw(),
    }
}

unsafe extern "C" fn create_pool(handle: Handle, family: u32, out: *mut u64) -> i32 {
    let Some(commands) = commands(handle) else {
        return vk::Result::ERROR_INITIALIZATION_FAILED.as_raw();
    };
    let mut pool = vk::CommandPool::null();
    let pool_result = commands.create_pool(family, vk::CommandPoolCreateFlags::TRANSIENT);
    let code = result(pool_result, &mut pool);
    unsafe { out.write(pool.as_raw()) };
    code
}

unsafe extern "C" fn destroy_pool(handle: Handle, pool: u64) {
    if let Some(commands) = commands(handle) {
        commands.destroy_pool(vk::CommandPool::from_raw(pool));
    }
}

unsafe extern "C" fn allocate(handle: Handle, pool: u64, out: *mut Handle) -> i32 {
    let Some(commands) = commands(handle) else {
        return vk::Result::ERROR_INITIALIZATION_FAILED.as_raw();
    };
    let mut buffer = vk::CommandBuffer::null();
    let code = result(
        commands.allocate(vk::CommandPool::from_raw(pool)),
        &mut buffer,
    );
    unsafe { out.write(buffer.as_raw() as Handle) };
    code
}

unsafe extern "C" fn reset_pool(handle: Handle, pool: u64) -> i32 {
    commands(handle)
        .map_or(vk::Result::ERROR_INITIALIZATION_FAILED, |commands| {
            commands.reset_pool(vk::CommandPool::from_raw(pool))
        })
        .as_raw()
}

fn buffer(commands: Handle) -> vk::CommandBuffer {
    vk::CommandBuffer::from_raw(commands as u64)
}

unsafe extern "C" fn begin(handle: Handle, commands: Handle) -> i32 {
    self::commands(handle)
        .map_or(vk::Result::ERROR_INITIALIZATION_FAILED, |ops| {
            ops.begin(buffer(commands))
        })
        .as_raw()
}

unsafe extern "C" fn end(handle: Handle, commands: Handle) -> i32 {
    self::commands(handle)
        .map_or(vk::Result::ERROR_INITIALIZATION_FAILED, |ops| {
            ops.end(buffer(commands))
        })
        .as_raw()
}

unsafe extern "C" fn create_fence(handle: Handle, out: *mut u64) -> i32 {
    let Some(commands) = commands(handle) else {
        return vk::Result::ERROR_INITIALIZATION_FAILED.as_raw();
    };
    let mut fence = vk::Fence::null();
    let code = result(commands.create_fence(), &mut fence);
    unsafe { out.write(fence.as_raw()) };
    code
}

unsafe extern "C" fn destroy_fence(handle: Handle, fence: u64) {
    if let Some(commands) = commands(handle) {
        commands.destroy_fence(vk::Fence::from_raw(fence));
    }
}

unsafe extern "C" fn reset_fence(handle: Handle, fence: u64) -> i32 {
    commands(handle)
        .map_or(vk::Result::ERROR_INITIALIZATION_FAILED, |commands| {
            commands.reset_fence(vk::Fence::from_raw(fence))
        })
        .as_raw()
}

unsafe extern "C" fn wait_fence(handle: Handle, fence: u64, timeout: u64) -> i32 {
    commands(handle)
        .map_or(vk::Result::ERROR_INITIALIZATION_FAILED, |commands| {
            commands.wait_fence(vk::Fence::from_raw(fence), timeout)
        })
        .as_raw()
}

unsafe extern "C" fn complete_readback(handle: Handle, commands: Handle) {
    unsafe { rsf_core_capture_vk_complete_group(native(handle).capture, commands as u64) };
}

unsafe extern "C" fn wants(handle: Handle, frame: u64) -> u32 {
    u32::from(unsafe { rsf_core_capture_vk_wants(native(handle).capture, frame) } != 0)
}

unsafe extern "C" fn readbacks(handle: Handle) -> u64 {
    unsafe { rsf_core_capture_vk_pending(native(handle).capture) }
}

unsafe extern "C" fn target(packet: *const Packet, out: *mut Target) {
    let packet = unsafe { &*packet };
    let info = present_info(packet);
    let chain = unsafe {
        &*packet
            .swapchain_info
            .cast::<vk::SwapchainCreateInfoKHR<'_>>()
    };
    let mut target = Target {
        usage: chain.image_usage.as_raw(),
        layers: chain.image_array_layers,
        format: chain.image_format.as_raw() as u32,
        width: chain.image_extent.width,
        height: chain.image_extent.height,
        ..Target::default()
    };
    if info.swapchain_count == 1 && !info.p_swapchains.is_null() && !info.p_image_indices.is_null()
    {
        target.chain = unsafe { *info.p_swapchains }.as_raw();
        target.index = unsafe { *info.p_image_indices };
        target.selected = 1;
    }
    unsafe { out.write(target) };
}

// The image query and the native Present bypass the SDK proxy under its LL2
// WSI lease.
fn wsi_call(packet: &Packet, call: &mut dyn FnMut() -> i32) -> i32 {
    unsafe extern "C" fn trampoline(user: *mut c_void) -> i32 {
        let call = unsafe { &mut *user.cast::<&mut dyn FnMut() -> i32>() };
        call()
    }
    let lease: WsiCall = unsafe { std::mem::transmute(packet.native_wsi_call) };
    let mut call = call;
    unsafe { lease(trampoline, (&raw mut call).cast()) }
}

unsafe extern "C" fn images(
    packet: *const Packet,
    chain: u64,
    count: *mut u32,
    out: *mut u64,
) -> i32 {
    let packet = unsafe { &*packet };
    let next: vk::PFN_vkGetSwapchainImagesKHR =
        unsafe { std::mem::transmute(packet.native_images) };
    let device = vk::Device::from_raw(packet.device as u64);
    let chain = vk::SwapchainKHR::from_raw(chain);
    wsi_call(packet, &mut || {
        unsafe { next(device, chain, count, out.cast()) }.as_raw()
    })
}

unsafe extern "C" fn record(
    handle: Handle,
    commands: Handle,
    image: *const Image,
    identity: *const Record,
    role: *const c_char,
) -> u32 {
    let reset = unsafe { (*identity).flags } & RSF_FRAME_FLAG_RESET;
    let recorded = unsafe {
        rsf_core_capture_vk_record(
            native(handle).capture,
            commands,
            image,
            identity,
            role,
            reset,
            0,
            ptr::null_mut(),
            commands as u64,
        )
    };
    u32::from(recorded != 0)
}

unsafe extern "C" fn submit(packet: *const Packet, commands: Handle, fence: u64) -> i32 {
    let packet = unsafe { &*packet };
    let info = present_info(packet);
    let stages = vec![vk::PipelineStageFlags::ALL_COMMANDS; info.wait_semaphore_count as usize];
    let buffer = buffer(commands);
    let submission = vk::SubmitInfo {
        wait_semaphore_count: info.wait_semaphore_count,
        p_wait_semaphores: info.p_wait_semaphores,
        p_wait_dst_stage_mask: stages.as_ptr(),
        command_buffer_count: 1,
        p_command_buffers: &buffer,
        ..Default::default()
    };
    let next: vk::PFN_vkQueueSubmit = unsafe { std::mem::transmute(packet.native_submit) };
    let queue = vk::Queue::from_raw(packet.queue as u64);
    unsafe { next(queue, 1, &submission, vk::Fence::from_raw(fence)) }.as_raw()
}

unsafe extern "C" fn present(packet: *const Packet, waits: u32) -> i32 {
    let packet = unsafe { &*packet };
    let mut info = *present_info(packet);
    if waits == 0 {
        info.wait_semaphore_count = 0;
        info.p_wait_semaphores = ptr::null();
    }
    let next: vk::PFN_vkQueuePresentKHR = unsafe { std::mem::transmute(packet.native_present) };
    let queue = vk::Queue::from_raw(packet.queue as u64);
    wsi_call(packet, &mut || unsafe { next(queue, &info) }.as_raw())
}

unsafe extern "C" fn flush(handle: Handle) -> u32 {
    let flushed = unsafe { rsf_core_capture_vk_flush(native(handle).capture, 0, ptr::null_mut()) };
    u32::from(flushed != 0)
}

fn append_bytes(handle: Handle, file: &str, bytes: &[u8]) {
    let path = native(handle).directory.join(file);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(bytes);
    }
}

unsafe extern "C" fn failure(handle: Handle, source: u64, reason: *const c_char, result: i32) {
    let reason = unsafe { CStr::from_ptr(reason) }.to_string_lossy();
    let line = format!("{source} {reason} {result}\n");
    append_bytes(handle, "dlss-present-capture-errors.log", line.as_bytes());
}

unsafe extern "C" fn append(handle: Handle, file: *const c_char, line: *const u8, length: usize) {
    let file = unsafe { CStr::from_ptr(file) }.to_string_lossy();
    let line = unsafe { std::slice::from_raw_parts(line, length) };
    append_bytes(handle, &file, line);
}

/// The production leaves. The native fixture copies the table and replaces
/// open and the frame, readback and file entries.
#[unsafe(no_mangle)]
pub(crate) static rsf_core_dlss_capture_leaves: Platform = Platform {
    open,
    close,
    register,
    create_pool,
    destroy_pool,
    allocate,
    reset_pool,
    begin,
    end,
    create_fence,
    destroy_fence,
    reset_fence,
    wait_fence,
    complete_readback,
    wants,
    readbacks,
    target,
    images,
    record,
    submit,
    present,
    flush,
    failure,
    append,
};

/// A capture native with the fixture's registration entry and no file output
/// or capture initialization, for the native present-capture fixture.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_capture_native(
    setup: *const Setup,
    register: Register,
) -> Handle {
    let Some(setup) = (unsafe { setup.as_ref() }) else {
        return ptr::null_mut();
    };
    let mut native = Native::new(setup, register);
    if !native.load_commands() {
        return ptr::null_mut();
    }
    Box::into_raw(native).cast()
}
