// SPDX-License-Identifier: MIT
//! Direct Unity D3D12 recording. Unity supplies its event, queue and frame-fence facts.

use crate::bridge_abi::{
    RSF_UNITY_FRAMES_SLOTS, RSF_UNITY_PACKET_HUDLESS, RSF_UNITY_PACKET_WINDOW,
    rsf_unity_frame_admission, rsf_unity_packet, rsf_unity_source_submission,
};
use crate::bridge_d3d12_abi::rsf_unity_d3d12_engine;
use crate::bridge_state::{
    rsf_unity_frames_activate, rsf_unity_frames_active, rsf_unity_frames_admit_frame,
    rsf_unity_frames_begin_d3d12_event, rsf_unity_frames_can_record_source,
    rsf_unity_frames_dispatch_d3d12, rsf_unity_frames_end_d3d12_event,
    rsf_unity_frames_flush_sources, rsf_unity_frames_record_source, rsf_unity_frames_submitted,
};
use crate::d3d11_pass::build_d3d12_pass;
use rsf_game_sys::{
    RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_WINDOW, rsf_game_host_services,
    rsf_game_render_pass,
};
use rsf_runtime_submission::{FenceStatus, fence_status};
use std::ffi::{CStr, c_void};
use std::ptr::null_mut;
use std::sync::{Mutex, PoisonError};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain3;
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::IDXGISwapChain};
use windows::core::Interface;

struct Commands {
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    leases: [Option<ID3D12Resource>; 4],
}

static SLOTS: Mutex<[Option<Commands>; RSF_UNITY_FRAMES_SLOTS as usize]> =
    Mutex::new([const { None }; RSF_UNITY_FRAMES_SLOTS as usize]);
static FENCE: Mutex<Option<ID3D12Fence>> = Mutex::new(None);

/// The event scope serializes slot access. Return ownership on every recording exit.
struct Slot {
    index: usize,
    commands: Option<Commands>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        SLOTS.lock().unwrap_or_else(PoisonError::into_inner)[self.index] = self.commands.take();
    }
}

struct Event;

impl Drop for Event {
    fn drop(&mut self) {
        rsf_unity_frames_end_d3d12_event();
    }
}

fn log(host: &rsf_game_host_services, text: &CStr) {
    if let Some(log) = host.log {
        unsafe { log(host.user, text.as_ptr()) };
    }
}

fn source(pass: &rsf_game_render_pass) -> rsf_unity_source_submission {
    rsf_unity_source_submission {
        session_id: pass.session_id,
        source_frame_id: pass.source_frame_id,
        view_key: pass.view_key,
    }
}

pub(crate) fn completed() -> u64 {
    let fence = FENCE.lock().unwrap_or_else(PoisonError::into_inner);
    fence
        .as_ref()
        .map_or(0, |fence| unsafe { fence.GetCompletedValue() })
}

pub(crate) fn release_graphics() {
    let slots = std::mem::take(&mut *SLOTS.lock().unwrap_or_else(PoisonError::into_inner));
    drop(slots);
    let fence = FENCE.lock().unwrap_or_else(PoisonError::into_inner).take();
    drop(fence);
}

unsafe fn frame_fence(engine: &rsf_unity_d3d12_engine) -> bool {
    // Only this event can populate it. Keep the Unity call outside the mutex.
    if FENCE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some()
    {
        return true;
    }
    let raw = unsafe { engine.frame_fence.unwrap()() };
    let Some(fence) = (unsafe { ID3D12Fence::from_raw_borrowed(&raw) }) else {
        return false;
    };
    *FENCE.lock().unwrap_or_else(PoisonError::into_inner) = Some(fence.clone());
    true
}

unsafe fn capture(
    engine: &rsf_unity_d3d12_engine,
    host: &rsf_game_host_services,
    mut packet: rsf_unity_packet,
) -> Option<()> {
    let fence = unsafe { frame_fence(engine) };
    let completion = unsafe { engine.next_frame_fence_value.unwrap()() };
    if !fence
        || completion == 0
        || completion == u64::MAX
        || fence_status(completed(), 0) != FenceStatus::Complete
    {
        return None;
    }
    let list = unsafe { engine.recording_list.unwrap()() };
    if list.is_null() {
        static REFUSALS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if REFUSALS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 3 {
            log(
                host,
                c"Unity pre-UI capture refused: no current recording list.",
            );
        }
        return None;
    }
    let back_buffer: ID3D12Resource;
    if packet.color.is_null() {
        let raw = unsafe { engine.swapchain.unwrap()() };
        let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&raw) }?;
        let chain = chain.cast::<IDXGISwapChain3>().ok()?;
        back_buffer = unsafe { chain.GetBuffer(chain.GetCurrentBackBufferIndex()) }.ok()?;
        packet.color = back_buffer.as_raw();
    }
    let mut pass = build_d3d12_pass(&packet);
    pass.role = RSF_GAME_RENDER_FINAL_SCENE;
    let recorded = source(&pass);
    if unsafe { rsf_unity_frames_can_record_source(&recorded) } == 0 {
        return None;
    }
    unsafe {
        host.render_pass.unwrap()(host.user, list, &pass, 1);
        rsf_unity_frames_record_source(&recorded, completion);
    }
    static REPORTS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if REPORTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 3 {
        let desc = unsafe { ID3D12Resource::from_raw_borrowed(&packet.color)?.GetDesc() };
        if let Ok(text) = std::ffi::CString::new(format!(
            "Unity pre-UI copy recorded on engine list: format={} {}x{}",
            desc.Format.0, desc.Width, desc.Height
        )) {
            log(host, &text);
        }
    }
    Some(())
}

unsafe fn reconstruct(
    engine: &rsf_unity_d3d12_engine,
    host: &rsf_game_host_services,
    device: &ID3D12Device,
    packet: &rsf_unity_packet,
    hudless: bool,
) -> Option<()> {
    if !unsafe { frame_fence(engine) } {
        return None;
    }
    let mut admission = rsf_unity_frame_admission {
        slot: 0,
        pending_generation: 0,
    };
    if unsafe { rsf_unity_frames_admit_frame(packet, completed(), &mut admission) } == 0 {
        return None;
    }
    let index = admission.slot as usize;
    let mut slot = Slot {
        index,
        commands: SLOTS.lock().unwrap_or_else(PoisonError::into_inner)[index].take(),
    };
    if let Some(commands) = slot.commands.as_mut() {
        commands.leases = Default::default();
    } else {
        let allocator =
            unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.ok()?;
        let list: ID3D12GraphicsCommandList = unsafe {
            device.CreateCommandList(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &allocator,
                None::<&ID3D12PipelineState>,
            )
        }
        .ok()?;
        unsafe { list.Close() }.ok()?;
        slot.commands = Some(Commands {
            allocator,
            list,
            leases: Default::default(),
        });
    }
    let commands = slot.commands.as_mut()?;
    let mut owner = None::<ID3D12Device>;
    if unsafe { commands.allocator.GetDevice(&mut owner) }.is_err()
        || owner.as_ref().map(Interface::as_raw) != Some(device.as_raw())
    {
        rsf_unity_frames_activate(0);
        log(
            host,
            c"Unity D3D12 device changed; adapter requires a drained restart.",
        );
        return None;
    }
    unsafe {
        commands.allocator.Reset().ok()?;
        commands
            .list
            .Reset(&commands.allocator, None::<&ID3D12PipelineState>)
            .ok()?;
    }
    let list = commands.list.as_raw();
    // A reset proves discard of any preceding recording that never reached Unity.
    #[cfg(not(feature = "mono-plugin"))]
    crate::package_state::rsf_unity_package_sr_slot_reset(12, list, admission.slot);
    let mut pass = build_d3d12_pass(packet);
    if hudless {
        pass.role = RSF_GAME_RENDER_FINAL_SCENE;
    }
    if !unsafe {
        rsf_unity_frames_dispatch_d3d12(host, list, &mut pass, admission.pending_generation)
    } {
        let _ = unsafe { commands.list.Close() };
        return None;
    }
    unsafe { commands.list.Close() }.ok()?;
    let mut resources = [null_mut(); 4];
    let mut states = [0; 4];
    let mut count = 0;
    // Extend the packet's recording references through Unity's GPU completion.
    for (index, raw) in [packet.color, packet.depth, packet.motion, packet.output]
        .into_iter()
        .enumerate()
    {
        if let Some(resource) = unsafe { ID3D12Resource::from_raw_borrowed(&raw) } {
            commands.leases[index] = Some(resource.clone());
            resources[count] = raw;
            states[count] = if hudless {
                D3D12_RESOURCE_STATE_RENDER_TARGET
            } else if index == 3 {
                D3D12_RESOURCE_STATE_COPY_DEST
            } else {
                D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE
            }
            .0;
            count += 1;
        }
    }
    let complete =
        unsafe { engine.execute.unwrap()(list, count as u32, resources.as_ptr(), states.as_ptr()) };
    if unsafe { rsf_unity_frames_submitted(&admission, complete, &source(&pass)) } == 0 {
        log(
            host,
            c"Unity D3D12 submission could not be tracked; adapter stopped and remains owned.",
        );
    }
    Some(())
}

pub(crate) unsafe fn render_packet(
    engine: &rsf_unity_d3d12_engine,
    host: &rsf_game_host_services,
    offset: u32,
    packet: &rsf_unity_packet,
) {
    if !rsf_unity_frames_begin_d3d12_event() {
        return;
    }
    let _event = Event;
    if rsf_unity_frames_active() == 0 || host.render_pass.is_none() {
        return;
    }
    let raw = unsafe { engine.device.unwrap()() };
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&raw) }) else {
        return;
    };
    let hudless = packet.flags & RSF_UNITY_PACKET_HUDLESS != 0;
    if offset == 2 && hudless {
        let _ = unsafe { capture(engine, host, *packet) };
    } else if offset == 1 && packet.flags & RSF_UNITY_PACKET_WINDOW != 0 {
        let mut pass = build_d3d12_pass(packet);
        pass.role = RSF_GAME_RENDER_WINDOW;
        pass.swapchain = unsafe { engine.swapchain.unwrap()() };
        let queue = unsafe { engine.queue.unwrap()() };
        // Unity synchronizes workers and flushes recorded lists before granting this event the queue.
        unsafe {
            rsf_unity_frames_flush_sources(host, queue, &pass);
            host.render_pass.unwrap()(host.user, queue, &pass, 1);
        }
    } else {
        let _ = unsafe { reconstruct(engine, host, device, packet, hudless) };
    }
}
