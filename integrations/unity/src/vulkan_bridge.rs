// SPDX-License-Identifier: MIT
//! Unity Vulkan frame routing. Foreign dispatch borrows first-party values through vulkan_foreign.h.

use super::*;
use crate::d3d11_pass::base_pass;
use crate::package_state::rsf_unity_package_bridge_failed;
use crate::package_state::services::{editor, rsf_unity_package_log};
use crate::sys::*;
use crate::sys::{RSF_UNITY_PACKET_PROBE, RSF_UNITY_PREVIEW_SOURCE_COMMON};
use crate::vulkan_platform::{commands::*, surface::*, *};
use crate::vulkan_resolver;
use ash::vk::{self, Handle as _};
use rsf_game_sys::{
    RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_RECONFIGURE, RSF_GAME_RENDER_WINDOW,
    backend_result, rsf_game_render_pass,
};
use std::ffi::CStr;
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
pub(crate) use window::{create_window, rsf_uvk_bridge_owns_window, rsf_uvk_bridge_update_window};

const EVENT_WINDOW: u32 = 1;
const QUEUE_EXCEPTION: &CStr = c"Unity Vulkan queue execution failed; resources remain owned.";
const PRESENT_EXCEPTION: &CStr = c"Unity Vulkan present interception failed; the bridge stopped.";
const SLOT_FAILED: &CStr = c"Unity Vulkan command slot could not be reused; the bridge stopped.";
const IMAGE_FAILED: &CStr = c"Unity Vulkan shared image creation failed; the bridge stopped.";

fn log(services: *const rsf_game_host_services, message: &CStr) {
    if unsafe { services.as_ref() }.is_some_and(|services| services.log.is_some()) {
        let services = unsafe { &*services };
        unsafe { rsf_game_log_guarded(services.log, services.user, message.as_ptr()) };
    } else {
        unsafe {
            OutputDebugStringA(message.as_ptr().cast());
            OutputDebugStringA(c"\n".as_ptr().cast());
        }
    }
}

fn fault(result: i32, message: &CStr) {
    if lock().fault() {
        rsf_unity_package_bridge_failed(result);
        unsafe { rsf_unity_package_log(message.as_ptr()) };
    }
}

fn stop(message: &CStr) {
    fault(backend_result::FEATURE_FAILED, message);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_uvk_bridge_retirement_fault() {
    stop(c"Unity Vulkan could not retire its present semaphores; ownership was retained.");
}

// vulkan_bridge.c: the loader lookups guarded against foreign exceptions.
unsafe extern "system" {
    fn rsf_uvk_foreign_instance_lookup(
        instance: vk::Instance,
        name: *const std::ffi::c_char,
    ) -> vk::PFN_vkVoidFunction;
    fn rsf_uvk_foreign_device_lookup(
        device: *mut c_void,
        name: *const std::ffi::c_char,
    ) -> vk::PFN_vkVoidFunction;
}

/// Fills the shared device from Unity's Vulkan instance: its Vulkan facts, the interop check and
/// the D3D12 objects on the same adapter.
pub(super) unsafe fn create_device(
    services: *const rsf_game_host_services,
    output: &mut DeviceHandles,
) -> bool {
    let Some(instance) = instance() else {
        return false;
    };
    if instance.instance == 0
        || instance.device == 0
        || instance.physical_device == 0
        || instance.queue == 0
    {
        return false;
    }
    output.vk_device = instance.device;
    output.vk_physical_device = instance.physical_device;
    output.vk_queue = instance.queue;
    output.vk_queue_family = instance.queue_family;
    if !vulkan_resolver::interop_ready(
        instance.device as *mut c_void,
        rsf_uvk_foreign_device_lookup,
    ) {
        log(
            services,
            c"Unity Vulkan device lacks enabled Win32 external-memory/semaphore extensions.",
        );
        return false;
    }
    // A VkDevice is the pointer-sized dispatchable handle the lookup takes.
    let gdpa = unsafe {
        std::mem::transmute::<vulkan_resolver::Lookup, vk::PFN_vkGetDeviceProcAddr>(
            rsf_uvk_foreign_device_lookup,
        )
    };
    let created = unsafe {
        rsf_uvk_gpu_create_device(
            output,
            instance.instance,
            rsf_uvk_foreign_instance_lookup,
            gdpa,
        )
    };
    if created == 2 {
        log(services, c"Unity Vulkan adapter supplied no Windows LUID.");
    }
    created == 1
}

pub(super) unsafe fn report_created(services: *const rsf_game_host_services) {
    log(
        services,
        c"Unity Vulkan textures share same-adapter D3D12 resources and GPU fences.",
    );
}

/// The device and surface leaves vulkan_state calls; its tests bind fakes with the same names.
pub(super) mod ops {
    pub(in crate::vulkan_state) use super::{create_device, report_created};
    pub(in crate::vulkan_state) use crate::package_state::{
        rsf_unity_package_sr_ring_begin as ring_begin,
        rsf_unity_package_sr_ring_retired as ring_retired,
    };
    pub(in crate::vulkan_state) use crate::vulkan_platform::surface::{
        rsf_uvk_gpu_chain_create as chain_create, rsf_uvk_gpu_chain_destroy as chain_destroy,
        rsf_uvk_gpu_chain_extent as chain_extent, rsf_uvk_gpu_chain_resize as chain_resize,
    };
    pub(in crate::vulkan_state) use crate::vulkan_platform::{
        rsf_uvk_gpu_create_image as create_image, rsf_uvk_gpu_create_pool as create_pool,
        rsf_uvk_gpu_create_slot as create_slot, rsf_uvk_gpu_destroy_device as destroy_device,
        rsf_uvk_gpu_destroy_image as destroy_image, rsf_uvk_gpu_destroy_pool as destroy_pool,
        rsf_uvk_gpu_destroy_slot as destroy_slot, rsf_uvk_gpu_drain as drain,
        rsf_uvk_gpu_fence_ready as fence_ready, rsf_uvk_gpu_shared_completed as shared_completed,
    };
}
fn initialize(services: *const rsf_game_host_services) -> Device {
    if !unsafe { rsf_uvk_foreign_available() } {
        return Device::Failed;
    }
    initialize_device(services)
}

struct Entry;
impl Entry {
    fn begin() -> Option<Self> {
        lock().enter().then_some(Self)
    }
}
impl Drop for Entry {
    fn drop(&mut self) {
        lock().leave();
    }
}

struct Lease;
impl Lease {
    fn try_begin() -> Option<Self> {
        try_begin_lease().then_some(Self)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        end_lease();
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

pub(crate) fn render_event(
    event_id: i32,
    base_event: i32,
    packet: &rsf_unity_packet,
    services: &rsf_game_host_services,
    reconfigure: bool,
) -> bool {
    let Some(_entry) = Entry::begin() else {
        return false;
    };
    let frame = Frame {
        packet: *packet,
        services: *services,
    };
    if !unsafe { rsf_uvk_foreign_available() } || frame.services.render_pass.is_none() || faulted()
    {
        return false;
    }
    let kind = match event_id.checked_sub(base_event) {
        Some(1) => EVENT_WINDOW,
        Some(2) => EVENT_HUDLESS,
        _ => 0,
    };
    if kind == EVENT_WINDOW && !editor() {
        let Some(instance) = instance() else {
            return false;
        };
        return publish_window(
            &frame,
            instance.instance,
            instance.device as *mut c_void,
            instance.queue,
            parent(),
        );
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
    unsafe { GetParent(surface_window()) }
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
    let key = ImageKey {
        width,
        height,
        format: format.as_raw() as u32,
        dx_format: dx_format.0 as u32,
    };
    super::prepare_image(&STATE, index as usize, key)
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
    let slot = command_slot(ring)?;
    unsafe { rsf_uvk_gpu_begin(graphics, &slot, ring) }.then_some((ring, slot))
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

pub(crate) fn graphics() -> Option<(ID3D12Device, ID3D12CommandQueue)> {
    let _entry = Entry::begin()?;
    let view = device_view()?;
    let device = unsafe { ID3D12Device::from_raw_borrowed(&view.device) }?.clone();
    let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&view.queue) }?.clone();
    Some((device, queue))
}
