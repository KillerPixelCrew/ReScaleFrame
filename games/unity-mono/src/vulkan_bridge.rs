// SPDX-License-Identifier: MIT
//! Unity Vulkan frame routing. Foreign dispatch borrows first-party values through vulkan_foreign.h.

use super::*;
use crate::bridge_abi::{RSF_UNITY_PACKET_PROBE, RSF_UNITY_PREVIEW_SOURCE_COMMON};
use crate::vulkan_foreign_abi::*;
use crate::vulkan_platform::{commands::*, surface::*, *};
use ash::vk::{self, Handle as _};
use rsf_game_sys::{
    RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_RECONFIGURE, RSF_GAME_RENDER_WINDOW,
    rsf_game_render_pass,
};
use std::ffi::{CStr, c_char};
use std::mem::MaybeUninit;
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::*};
use windows::core::Interface;
use windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringA;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetParent, SW_HIDE, SW_SHOWNOACTIVATE, ShowWindowAsync,
};

#[path = "vulkan_bridge/present.rs"]
mod present;
#[path = "vulkan_bridge/queue.rs"]
mod queue;
#[path = "vulkan_bridge/window.rs"]
mod window;

const EVENT_WINDOW: u32 = 1;
// The C backend status values are cast macros.
const INIT_FAILED: i32 = -6;
const FEATURE_FAILED: i32 = -9;
const QUEUE_EXCEPTION: &CStr = c"Unity Vulkan queue execution failed; resources remain owned.";
const PRESENT_EXCEPTION: &CStr = c"Unity Vulkan present interception failed; the bridge stopped.";
const SLOT_FAILED: &CStr = c"Unity Vulkan command slot could not be reused; the bridge stopped.";
const IMAGE_FAILED: &CStr = c"Unity Vulkan shared image creation failed; the bridge stopped.";

unsafe extern "C" {
    fn rsf_unity_package_bridge_failed(result: i32);
    fn rsf_unity_package_log(text: *const c_char);
    fn rsf_unity_package_editor() -> u32;
    fn rsf_unity_package_sr_ring_begin() -> u64;
    fn rsf_unity_package_sr_ring_retired(epoch: u64);
    fn rsf_unity_build_scalar_pass(
        packet: *const rsf_unity_packet,
        pass: *mut rsf_game_render_pass,
    );
    fn rsf_unity_vk_interop_ready(device: vk::Device, lookup: vk::PFN_vkGetDeviceProcAddr) -> bool;
}

extern "C" fn ring_begin() -> u64 {
    unsafe { rsf_unity_package_sr_ring_begin() }
}

extern "C" fn ring_retired(epoch: u64) {
    unsafe { rsf_unity_package_sr_ring_retired(epoch) }
}

fn log(services: *const rsf_game_host_services, message: &CStr) {
    if unsafe { services.as_ref() }.is_some_and(|services| services.log.is_some()) {
        unsafe { rsf_uvk_foreign_log(services, message.as_ptr()) };
    } else {
        unsafe {
            OutputDebugStringA(message.as_ptr().cast());
            OutputDebugStringA(c"\n".as_ptr().cast());
        }
    }
}

fn fault(result: i32, message: &CStr) {
    if rsf_unity_vulkan_fault() != 0 {
        unsafe {
            rsf_unity_package_bridge_failed(result);
            rsf_unity_package_log(message.as_ptr());
        }
    }
}

fn stop(message: &CStr) {
    fault(FEATURE_FAILED, message);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_uvk_bridge_retirement_fault() {
    stop(c"Unity Vulkan could not retire its present semaphores; ownership was retained.");
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_bridge_create_device(
    services: *const rsf_game_host_services,
    instance: *const rsf_uvk_engine_instance,
    output: *mut DeviceHandles,
    gipa: unsafe extern "C" fn(),
    gdpa: unsafe extern "C" fn(),
) -> bool {
    let instance = unsafe { &*instance };
    if instance.instance == 0
        || instance.device == 0
        || instance.physical_device == 0
        || instance.queue == 0
    {
        return false;
    }
    let output = unsafe { &mut *output };
    output.vk_device = instance.device;
    output.vk_physical_device = instance.physical_device;
    output.vk_queue = instance.queue;
    output.vk_queue_family = instance.queue_family;
    // The native loader supplies these two exact Vulkan entry points on Windows x64.
    let gipa = unsafe { std::mem::transmute::<_, vk::PFN_vkGetInstanceProcAddr>(gipa) };
    let gdpa = unsafe { std::mem::transmute::<_, vk::PFN_vkGetDeviceProcAddr>(gdpa) };
    if !unsafe { rsf_unity_vk_interop_ready(vk::Device::from_raw(instance.device), gdpa) } {
        log(
            services,
            c"Unity Vulkan device lacks enabled Win32 external-memory/semaphore extensions.",
        );
        return false;
    }
    let created = unsafe { rsf_uvk_gpu_create_device(output, instance.instance, gipa, gdpa) };
    if created == 2 {
        log(services, c"Unity Vulkan adapter supplied no Windows LUID.");
    }
    created == 1
}

unsafe extern "C" fn report_created(services: *const rsf_game_host_services) {
    log(
        services,
        c"Unity Vulkan textures share same-adapter D3D12 resources and GPU fences.",
    );
}

static DEVICE_OPS: DeviceOps = DeviceOps {
    create: rsf_uvk_foreign_create_device,
    destroy: rsf_uvk_gpu_destroy_device,
    create_pool: rsf_uvk_gpu_create_pool,
    destroy_pool: rsf_uvk_gpu_destroy_pool,
    create_slot: rsf_uvk_gpu_create_slot,
    destroy_slot: rsf_uvk_gpu_destroy_slot,
    create_image: rsf_uvk_gpu_create_image,
    destroy_image: rsf_uvk_gpu_destroy_image,
    fence_ready: rsf_uvk_gpu_fence_ready,
    shared_completed: rsf_uvk_gpu_shared_completed,
    drain_images: rsf_uvk_gpu_drain,
    report_created,
    ring_begin,
    ring_retired,
};

static SURFACE_OPS: SurfaceOps = SurfaceOps {
    create: rsf_uvk_gpu_chain_create,
    extent: rsf_uvk_gpu_chain_extent,
    drain: rsf_uvk_gpu_drain,
    resize: rsf_uvk_gpu_chain_resize,
    destroy: rsf_uvk_gpu_chain_destroy,
};

fn initialize(services: *const rsf_game_host_services, graphics: &mut DeviceHandles) -> u32 {
    if !unsafe { rsf_uvk_foreign_available() } {
        return DEVICE_FAILED;
    }
    unsafe { rsf_unity_vulkan_initialize(services, &DEVICE_OPS, graphics) }
}

fn configure_surface(width: u32, height: u32) -> *mut c_void {
    unsafe { rsf_unity_vulkan_configure_surface(width, height, &SURFACE_OPS) }
}

struct Entry;
impl Entry {
    fn begin() -> Option<Self> {
        (rsf_unity_vulkan_enter() != 0).then(|| Self)
    }
}
impl Drop for Entry {
    fn drop(&mut self) {
        rsf_unity_vulkan_leave();
    }
}

struct Lease;
impl Lease {
    fn try_begin() -> Option<Self> {
        (rsf_unity_vulkan_presentation_try_begin() != 0).then(|| Self)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        rsf_unity_vulkan_presentation_end();
    }
}

struct RunningEvent(usize);
impl Drop for RunningEvent {
    fn drop(&mut self) {
        lock().finish_event(self.0);
    }
}

#[derive(Clone, Copy)]
pub(super) struct Textures([rsf_uvk_engine_image; 4]);
impl Textures {
    pub(super) const EMPTY: Self = Self(
        [rsf_uvk_engine_image {
            image: 0,
            layout: 0,
            aspect: 0,
        }; 4],
    );
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_bridge_render(
    event_id: i32,
    base_event: i32,
    packet: *const rsf_unity_packet,
    services: *const rsf_game_host_services,
    reconfigure: bool,
) -> bool {
    let Some(_entry) = Entry::begin() else {
        return false;
    };
    let frame = unsafe {
        Frame {
            packet: *packet,
            services: *services,
        }
    };
    if !unsafe { rsf_uvk_foreign_available() }
        || frame.services.render_pass.is_none()
        || rsf_unity_vulkan_faulted() != 0
    {
        return false;
    }
    let kind = match event_id.checked_sub(base_event) {
        Some(1) => EVENT_WINDOW,
        Some(2) => EVENT_HUDLESS,
        _ => 0,
    };
    if kind == EVENT_WINDOW && unsafe { rsf_unity_package_editor() } == 0 {
        let Some(instance) = instance() else {
            return false;
        };
        return unsafe {
            rsf_unity_vulkan_set_window(
                &frame,
                instance.instance,
                instance.device as *mut c_void,
                instance.queue,
                parent(),
            )
        } != 0;
    }
    let mut textures = Textures::EMPTY;
    for (index, (texture, view)) in [
        frame.packet.color,
        frame.packet.depth,
        frame.packet.motion,
        frame.packet.output,
    ]
    .into_iter()
    .zip(&mut textures.0)
    .enumerate()
    {
        if texture.is_null() {
            continue;
        }
        let (layout, access) = if index == 3 {
            (
                vk::ImageLayout::GENERAL,
                vk::AccessFlags::TRANSFER_READ | vk::AccessFlags::TRANSFER_WRITE,
            )
        } else {
            (
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                vk::AccessFlags::TRANSFER_READ,
            )
        };
        if !unsafe { rsf_uvk_foreign_texture(texture, layout.as_raw(), access.as_raw(), view) } {
            log(&frame.services, c"Unity Vulkan texture access was refused.");
            return false;
        }
    }
    if textures.0[0].image == 0 {
        return false;
    }
    let address = {
        let mut state = lock();
        let Some(slot) = state.admit_event(&frame, kind, reconfigure) else {
            return false;
        };
        state.events[slot].textures = textures;
        state.event_address(slot)
    };
    if unsafe { rsf_uvk_foreign_queue(event_id, address) } {
        return true;
    }
    // Unity may have taken the address before throwing. Keep the queued event owned until
    // the callback observes the fault instead of recycling its identity prematurely.
    stop(QUEUE_EXCEPTION);
    false
}

fn instance() -> Option<rsf_uvk_engine_instance> {
    let mut info = rsf_uvk_engine_instance::default();
    unsafe { rsf_uvk_foreign_instance(&mut info) }.then_some(info)
}

fn parent() -> *mut c_void {
    unsafe { GetParent(rsf_unity_vulkan_surface_window()) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_uvk_bridge_execute(address: *const c_void) {
    let (slot, frame, kind, reconfigure, textures) = {
        let mut state = lock();
        let Some((slot, frame, kind, reconfigure)) = state.start_event(address) else {
            return;
        };
        (slot, frame, kind, reconfigure, state.events[slot].textures)
    };
    let _event = RunningEvent(slot);
    let Some(_lease) = Lease::try_begin() else {
        return;
    };
    if let Err(message) = queue::execute(frame, kind, reconfigure, &textures.0) {
        stop(message);
    }
}

fn prepare_image(
    index: u32,
    width: u32,
    height: u32,
    format: vk::Format,
    dx_format: DXGI_FORMAT,
) -> Option<ImageHandles> {
    let mut image = ImageHandles::EMPTY;
    unsafe {
        rsf_unity_vulkan_prepare_image(
            index,
            width,
            height,
            format.as_raw() as u32,
            dx_format.0 as u32,
            &mut image,
        )
    }
    .then_some(image)
}

fn prepare_rgba(index: u32, width: u32, height: u32) -> Option<ImageHandles> {
    prepare_image(
        index,
        width,
        height,
        vk::Format::R8G8B8A8_UNORM,
        DXGI_FORMAT_R8G8B8A8_UNORM,
    )
}

fn begin(graphics: &DeviceHandles) -> Option<(u32, CommandSlot)> {
    let ring = VULKAN.lock().slot().0 as u32;
    let mut slot = CommandSlot::EMPTY;
    unsafe {
        rsf_unity_vulkan_command_slot(ring, &mut slot) && rsf_uvk_gpu_begin(graphics, &slot, ring)
    }
    .then_some((ring, slot))
}

fn pass(frame: &Frame) -> rsf_game_render_pass {
    let mut pass = MaybeUninit::uninit();
    // The producer writes the full scalar pass for every non-null packet.
    unsafe {
        rsf_unity_build_scalar_pass(&frame.packet, pass.as_mut_ptr());
        pass.assume_init()
    }
}

fn render(frame: &Frame, target: *mut c_void, pass: &rsf_game_render_pass, phase: u32) -> bool {
    unsafe { rsf_uvk_foreign_pass(&frame.services, target, pass, phase) }
}

fn record_input(
    graphics: &DeviceHandles,
    command: u64,
    index: u32,
    view: ImageHandles,
    source: rsf_uvk_engine_image,
    size: [u32; 2],
) {
    unsafe {
        rsf_uvk_gpu_image_transition(graphics, command, view.image, index, false, true);
        rsf_uvk_gpu_copy(
            graphics,
            command,
            source.image,
            source.layout,
            source.aspect,
            view.image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL.as_raw(),
            size[0],
            size[1],
        );
        rsf_uvk_gpu_image_transition(graphics, command, view.image, 0, false, false);
    }
}

fn barrier(
    slot: &CommandSlot,
    resource: *mut c_void,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
    unsafe { rsf_uvk_gpu_dx_barrier(slot.commands, resource, before.0, after.0) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_bridge_graphics(
    device: *mut *mut c_void,
    queue: *mut *mut c_void,
) -> bool {
    let Some(_entry) = Entry::begin() else {
        return false;
    };
    let mut view = DeviceHandles::EMPTY;
    if !unsafe { rsf_unity_vulkan_device_view(&mut view) } {
        return false;
    }
    let native_device = unsafe { ID3D12Device::from_raw_borrowed(&view.device) };
    let native_queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&view.queue) };
    let (Some(native_device), Some(native_queue)) = (native_device, native_queue) else {
        return false;
    };
    unsafe {
        device.write(native_device.clone().into_raw());
        queue.write(native_queue.clone().into_raw());
    }
    true
}
