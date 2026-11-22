// SPDX-License-Identifier: MIT
//! The Unity package's D3D11-to-D3D12 recording and hand-back.

use super::*;
use crate::bridge_abi::rsf_unity_packet;
use crate::bridge_state::{rsf_unity_frames_activate, rsf_unity_frames_active};
use crate::d3d11_pass::{
    self, HANDBACK_FAILED, PassPlan, RETIRED, ROUTE_NONE, ROUTE_PREVIEW, ROUTE_WINDOW,
};
use crate::shared_surface_abi::*;
use rsf_game_sys::rsf_game_render_pass;
use rsf_runtime_submission::{FenceStatus, fence_status};
use std::ffi::{CStr, c_char};
use std::mem::ManuallyDrop;
use std::ptr::{null, null_mut};
use windows::Win32::{
    Foundation::HANDLE,
    Graphics::{
        Direct3D::D3D_FEATURE_LEVEL_11_0,
        Direct3D11::*,
        Direct3D12::*,
        Dxgi::{IDXGIDevice, IDXGISwapChain},
    },
};
use windows::core::{IUnknown, Interface};
use windows_sys::Win32::{
    Foundation::{CloseHandle, WAIT_OBJECT_0},
    System::Threading::{CreateEventW, WaitForSingleObject},
};

// The linked Runtime shared-surface API defines RSF_SHARED_OK as a cast macro.
const RSF_SHARED_OK: rsf_shared_result = 0;

unsafe extern "C" {
    fn rsf_unity_d3d11_engine_device() -> *mut c_void;
    fn rsf_unity_d3d11_engine_swapchain() -> *mut c_void;
    fn rsf_unity_d3d11_runtime_graphics(device: *mut *mut c_void, queue: *mut *mut c_void) -> bool;
    fn rsf_unity_d3d11_report_creation_failure(message: *const c_char);
    fn rsf_unity_package_sr_slot_reset(bridge: u32, list: *mut c_void, slot: u32);
}

unsafe fn borrowed<T: Interface>(raw: &*mut c_void) -> &T {
    // The published graphics record or a surface lease keeps this reference live.
    unsafe { T::from_raw_borrowed(raw) }.expect("live Unity bridge interface")
}

unsafe fn release(raw: *mut c_void) {
    if !raw.is_null() {
        drop(unsafe { IUnknown::from_raw(raw) });
    }
}

unsafe fn prepare_device(host: &rsf_game_host_services, out: &mut DeviceHandles) -> Option<()> {
    let raw = unsafe { rsf_unity_d3d11_engine_device() };
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }?.clone();
    let context = unsafe { device.GetImmediateContext() }.ok()?;
    let device11 = device.cast::<ID3D11Device5>().ok()?;
    let context11 = context.cast::<ID3D11DeviceContext4>().ok()?;
    out.device11 = device11.into_raw();
    out.context11 = context11.into_raw();
    let (mut device12, mut queue12) = (null_mut(), null_mut());
    if unsafe { rsf_unity_d3d11_runtime_graphics(&mut device12, &mut queue12) } {
        // Shared textures and command lists use the Runtime's native device and queue.
        let device = unsafe { ID3D12Device::from_raw_borrowed(&device12) }?.clone();
        let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue12) }?.clone();
        out.device12 = device.into_raw();
        out.queue12 = queue.into_raw();
    } else {
        let adapter = unsafe { device.cast::<IDXGIDevice>().ok()?.GetAdapter() }.ok()?;
        let mut device = None::<ID3D12Device>;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.ok()?;
        out.device12 = device?.into_raw();
        let device = unsafe { borrowed::<ID3D12Device>(&out.device12) };
        let queue: ID3D12CommandQueue =
            unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC::default()) }.ok()?;
        out.queue12 = queue.into_raw();
    }

    let mut sharing = null_mut();
    let shared = unsafe {
        rsf_shared_fence_create(
            out.device11,
            out.device12,
            host.log,
            host.user,
            &mut sharing,
        )
    };
    out.sharing = sharing.cast();
    if shared != RSF_SHARED_OK {
        return None;
    }
    let device = unsafe { borrowed::<ID3D12Device>(&out.device12) };
    let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.ok()?;
    out.completion = fence.into_raw();
    Some(())
}

unsafe extern "C" fn create(host: *const rsf_game_host_services, out: *mut DeviceHandles) -> bool {
    unsafe { prepare_device(&*host, &mut *out) }.is_some()
}

unsafe extern "C" fn destroy(shared: *const DeviceHandles) {
    let shared = unsafe { &*shared };
    unsafe {
        rsf_shared_fence_destroy(shared.sharing.cast());
        release(shared.completion);
        release(shared.queue12);
        release(shared.device12);
        release(shared.context11);
        release(shared.device11);
    }
}

unsafe fn prepare_slot(shared: &DeviceHandles, out: &mut CommandSlot) -> Option<()> {
    let device = unsafe { borrowed::<ID3D12Device>(&shared.device12) };
    let allocator: ID3D12CommandAllocator =
        unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.ok()?;
    out.allocator = allocator.into_raw();
    let list: ID3D12GraphicsCommandList = unsafe {
        device.CreateCommandList(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            borrowed::<ID3D12CommandAllocator>(&out.allocator),
            None::<&ID3D12PipelineState>,
        )
    }
    .ok()?;
    out.list = list.into_raw();
    unsafe { borrowed::<ID3D12GraphicsCommandList>(&out.list).Close() }.ok()
}

unsafe extern "C" fn create_slot(shared: *const DeviceHandles, out: *mut CommandSlot) -> bool {
    unsafe { prepare_slot(&*shared, &mut *out) }.is_some()
}

unsafe extern "C" fn destroy_slot(slot: *const CommandSlot) {
    let slot = unsafe { &*slot };
    unsafe {
        release(slot.list);
        release(slot.allocator);
    }
}

unsafe extern "C" fn completed(shared: *const DeviceHandles) -> u64 {
    let shared = unsafe { &*shared };
    unsafe { ID3D12Fence::from_raw_borrowed(&shared.completion) }
        .map_or(0, |fence| unsafe { fence.GetCompletedValue() })
}

unsafe extern "C" fn shared_completed(shared: *const DeviceHandles) -> u64 {
    let raw = unsafe { rsf_shared_fence_d3d11((*shared).sharing.cast()) };
    unsafe { ID3D11Fence::from_raw_borrowed(&raw) }
        .map_or(0, |fence| unsafe { fence.GetCompletedValue() })
}

unsafe extern "C" fn wait(shared: *const DeviceHandles, value: u64) -> bool {
    let shared = unsafe { &*shared };
    let Some(fence) = (unsafe { ID3D12Fence::from_raw_borrowed(&shared.completion) }) else {
        return false;
    };
    match fence_status(unsafe { fence.GetCompletedValue() }, value) {
        FenceStatus::Complete => return true,
        FenceStatus::Removed => return false,
        FenceStatus::Pending => {}
    }
    let event = unsafe { CreateEventW(null(), 0, 0, null()) };
    if event.is_null() {
        return false;
    }
    let ready = unsafe {
        fence.SetEventOnCompletion(value, HANDLE(event)).is_ok()
            && WaitForSingleObject(event, 10000) == WAIT_OBJECT_0
    };
    unsafe { CloseHandle(event) };
    ready && fence_status(unsafe { fence.GetCompletedValue() }, value) == FenceStatus::Complete
}

unsafe extern "C" fn create_surface(
    shared: *const DeviceHandles,
    host: *const rsf_game_host_services,
    width: u32,
    height: u32,
    format: u32,
    out: *mut *mut c_void,
) -> bool {
    let shared = unsafe { &*shared };
    let host = unsafe { host.as_ref() };
    let setup = rsf_shared_surface_setup {
        struct_size: size_of::<rsf_shared_surface_setup>() as u32,
        abi_version: RSF_SHARED_SURFACE_ABI_VERSION,
        width,
        height,
        format,
        render_target: 1,
        log: host.and_then(|host| host.log),
        log_user: host.map_or(null_mut(), |host| host.user),
        unordered_access: 0,
    };
    let mut surface = null_mut();
    let result = unsafe {
        rsf_shared_surface_create(shared.device11, shared.device12, &setup, &mut surface)
    };
    unsafe { out.write(surface.cast()) };
    result == RSF_SHARED_OK
}

unsafe extern "C" fn destroy_surface(surface: *mut c_void) {
    unsafe { rsf_shared_surface_destroy(surface.cast()) };
}

const OPS: DeviceOps = DeviceOps {
    create,
    destroy,
    create_slot,
    destroy_slot,
    completed,
    shared_completed,
    wait,
    create_surface,
    destroy_surface,
};

struct SurfaceLease;

impl SurfaceLease {
    fn acquire() -> Option<Self> {
        rsf_unity_d3d11_surfaces_begin().then_some(Self)
    }
}

impl Drop for SurfaceLease {
    fn drop(&mut self) {
        rsf_unity_d3d11_surfaces_end();
    }
}

struct Recording<'a>(Option<&'a ID3D12GraphicsCommandList>);

impl Recording<'_> {
    fn close(&mut self) -> Option<()> {
        unsafe { self.0.take()?.Close() }.ok()
    }
}

impl Drop for Recording<'_> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn log(host: &rsf_game_host_services, text: &CStr) {
    if let Some(log) = host.log {
        unsafe { log(host.user, text.as_ptr()) };
    }
}

unsafe fn surface(
    index: u32,
    texture: &ID3D11Texture2D,
    host: &rsf_game_host_services,
) -> Option<*mut rsf_shared_surface> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    if desc.MipLevels != 1
        || desc.ArraySize != 1
        || desc.SampleDesc.Count != 1
        || desc.SampleDesc.Quality != 0
    {
        return None;
    }
    let mut raw = null_mut();
    unsafe {
        rsf_unity_d3d11_prepare_surface(
            index,
            host,
            desc.Width,
            desc.Height,
            desc.Format.0 as u32,
            &mut raw,
        )
    }
    .then_some(raw.cast())
}

unsafe fn barrier(
    list: &ID3D12GraphicsCommandList,
    raw: *mut c_void,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
    if raw.is_null() || before == after {
        return;
    }
    let transition = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: ManuallyDrop::new(Some(unsafe { ID3D12Resource::from_raw(raw) })),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
        ..Default::default()
    };
    unsafe { list.ResourceBarrier(&[transition]) };
}

struct Interop<'a> {
    device: &'a DeviceHandles,
    context: &'a ID3D11DeviceContext4,
    queue: &'a ID3D12CommandQueue,
    fence11: &'a ID3D11Fence,
    fence12: &'a ID3D12Fence,
    host: &'a rsf_game_host_services,
}

impl Interop<'_> {
    fn pass(&self, commands: *mut c_void, pass: &rsf_game_render_pass, count: u32) {
        unsafe { self.host.render_pass.unwrap()(self.host.user, commands, pass, count) };
    }

    fn signal_copy(&self, value: Option<u64>, failure: &CStr) -> Option<()> {
        if let Some(value) = value
            && unsafe { self.context.Signal(self.fence11, value) }.is_ok()
        {
            unsafe { self.context.Flush() };
            D3D11.lock().copy_submitted(value);
            return Some(());
        }
        rsf_unity_frames_activate(0);
        log(self.host, failure);
        None
    }

    fn hand_back(&self, value: u64) -> bool {
        unsafe {
            self.queue.Signal(self.fence12, value).is_ok()
                && self.context.Wait(self.fence11, value).is_ok()
        }
    }

    fn retire(&self, completion: u64, handed_back: bool) -> Option<()> {
        // Publish the completion signal even when the D3D11 hand-back failed.
        let signalled = unsafe {
            self.queue
                .Signal(borrowed::<ID3D12Fence>(&self.device.completion), completion)
        }
        .is_ok();
        match d3d11_pass::retire_submission(signalled, handed_back) {
            RETIRED => Some(()),
            HANDBACK_FAILED => {
                log(
                    self.host,
                    c"Unity DX11 shared-surface hand-back failed; adapter stopped.",
                );
                None
            }
            _ => {
                log(
                    self.host,
                    c"Unity DX11 completion signal failed; adapter stopped.",
                );
                None
            }
        }
    }

    unsafe fn window(&self, mut plan: PassPlan, packet: &rsf_unity_packet) -> Option<()> {
        plan.pass.swapchain = unsafe { rsf_unity_d3d11_engine_swapchain() };
        if plan.route == ROUTE_WINDOW {
            self.pass(self.queue.as_raw(), &plan.pass, 1);
            return Some(());
        }
        let _lease = SurfaceLease::acquire()?;
        let source = unsafe { IUnknown::from_raw_borrowed(&packet.color) }?
            .cast::<ID3D11Texture2D>()
            .ok()?;
        let preview = unsafe { surface(plan.surfaces[0], &source, self.host) }?;
        let (uploaded, consumed) = D3D11.lock().shared_pair()?;
        D3D11.lock().record_copy();
        let raw = unsafe { rsf_shared_surface_d3d11(preview) };
        unsafe {
            self.context
                .CopyResource(borrowed::<ID3D11Texture2D>(&raw), &source)
        };
        self.signal_copy(Some(uploaded), c"Unity DX11 preview upload has no completion signal; adapter stopped and remains owned.")?;
        unsafe { self.queue.Wait(self.fence12, uploaded) }.ok()?;
        plan.pass.color_input = unsafe { rsf_shared_surface_d3d12(preview) };
        let completion = D3D11.lock().completion()?;
        self.pass(self.queue.as_raw(), &plan.pass, 1);
        self.retire(completion, self.hand_back(consumed))
    }

    unsafe fn record(&self, mut plan: PassPlan, packet: &rsf_unity_packet) -> Option<()> {
        // Keep every surface live through output copy and the closing pass notification.
        let _lease = SurfaceLease::acquire()?;
        let (index, retired) = D3D11.lock().slot();
        let mut slot = CommandSlot::EMPTY;
        if !unsafe { rsf_unity_d3d11_command_slot(index as u32, &mut slot) } {
            return None;
        }
        if !unsafe { wait(self.device, retired) } {
            log(self.host, c"Unity DX11 command-slot retirement timed out.");
            return None;
        }
        let allocator = unsafe { borrowed::<ID3D12CommandAllocator>(&slot.allocator) };
        let list = unsafe { borrowed::<ID3D12GraphicsCommandList>(&slot.list) };
        unsafe {
            allocator.Reset().ok()?;
            list.Reset(allocator, None::<&ID3D12PipelineState>).ok()?;
        }
        let mut recording = Recording(Some(list));
        let mut inputs: [Option<ID3D11Texture2D>; 4] = Default::default();
        let mut shared = [null_mut(); 4];
        if plan.swapchain_color != 0 {
            let raw = unsafe { rsf_unity_d3d11_engine_swapchain() };
            let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&raw) }?;
            inputs[0] = Some(unsafe { chain.GetBuffer(0) }.ok()?);
        }
        // Resolve every compatible surface before the first GPU copy.
        for (i, raw) in [packet.color, packet.depth, packet.motion, packet.output]
            .into_iter()
            .enumerate()
        {
            if let Some(source) = unsafe { IUnknown::from_raw_borrowed(&raw) } {
                inputs[i] = Some(source.cast::<ID3D11Texture2D>().ok()?);
            }
            if let Some(input) = &inputs[i] {
                shared[i] = unsafe { surface(plan.surfaces[i], input, self.host) }?;
            }
        }
        let (upload, download) = D3D11.lock().shared_pair()?;
        for (surface, input) in shared.into_iter().zip(&inputs) {
            if let Some(input) = input {
                D3D11.lock().record_copy();
                let raw = unsafe { rsf_shared_surface_d3d11(surface) };
                unsafe {
                    self.context
                        .CopyResource(borrowed::<ID3D11Texture2D>(&raw), input)
                };
            }
        }
        self.signal_copy(
            Some(upload),
            c"Unity DX11 upload has no completion signal; adapter stopped and remains owned.",
        )?;
        unsafe { self.queue.Wait(self.fence12, upload) }.ok()?;
        let resources = shared.map(|surface| {
            if surface.is_null() {
                null_mut()
            } else {
                unsafe { rsf_shared_surface_d3d12(surface) }
            }
        });
        for (raw, state) in resources.into_iter().zip(plan.states) {
            unsafe {
                barrier(
                    list,
                    raw,
                    D3D12_RESOURCE_STATE_COMMON,
                    D3D12_RESOURCE_STATES(state as i32),
                )
            };
        }
        [
            plan.pass.color_input,
            plan.pass.depth,
            plan.pass.motion,
            plan.pass.color_output,
        ] = resources;
        if !d3d11_pass::apply_pending_change(&mut plan) {
            return None;
        }
        unsafe { rsf_unity_package_sr_slot_reset(11, slot.list, index as u32) };
        self.pass(slot.list, &plan.pass, 1);
        for (raw, state) in resources.into_iter().zip(plan.states) {
            unsafe {
                barrier(
                    list,
                    raw,
                    D3D12_RESOURCE_STATES(state as i32),
                    D3D12_RESOURCE_STATE_COMMON,
                )
            };
        }
        recording.close()?;
        let completion = D3D11.lock().submit(index)?;
        let list = ManuallyDrop::new(Some(unsafe { ID3D12CommandList::from_raw(slot.list) }));
        unsafe { self.queue.ExecuteCommandLists(std::slice::from_ref(&*list)) };
        let handed_back = self.hand_back(download);
        if handed_back
            && plan.copy_back != 0
            && let Some(output) = &inputs[3]
        {
            D3D11.lock().record_copy();
            let raw = unsafe { rsf_shared_surface_d3d11(shared[3]) };
            unsafe {
                self.context
                    .CopyResource(output, borrowed::<ID3D11Texture2D>(&raw))
            };
            let tail = D3D11.lock().shared_tail();
            let _ = self.signal_copy(tail, c"Unity DX11 output copy has no completion signal; adapter stopped and remains owned.");
        }
        self.retire(completion, handed_back)?;
        self.pass(slot.list, &plan.pass, 0);
        D3D11.lock().advance(index);
        Some(())
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_d3d11_render_packet(
    host: *const rsf_game_host_services,
    offset: u32,
    editor: bool,
    packet: *const rsf_unity_packet,
) {
    let (host, packet) = unsafe { (&*host, &*packet) };
    if rsf_unity_frames_active() == 0 {
        return;
    }
    let mut device = DeviceHandles::EMPTY;
    let ready = unsafe { rsf_unity_d3d11_initialize(host, &OPS, &mut device) };
    if ready == DEVICE_FAILED {
        unsafe {
            rsf_unity_d3d11_report_creation_failure(c"Unity DX11 shared device/fence initialization failed; native output remains in place.".as_ptr());
        }
    }
    if ready != DEVICE_READY || host.render_pass.is_none() {
        return;
    }
    let f11 = unsafe { rsf_shared_fence_d3d11(device.sharing.cast()) };
    let f12 = unsafe { rsf_shared_fence_d3d12(device.sharing.cast()) };
    let interop = Interop {
        device: &device,
        host,
        context: unsafe { borrowed(&device.context11) },
        queue: unsafe { borrowed(&device.queue12) },
        fence11: unsafe { borrowed(&f11) },
        fence12: unsafe { borrowed(&f12) },
    };
    let plan = d3d11_pass::plan(offset, packet, editor);
    match plan.route {
        ROUTE_NONE => {}
        ROUTE_WINDOW | ROUTE_PREVIEW => {
            let _ = unsafe { interop.window(plan, packet) };
        }
        _ => {
            let _ = unsafe { interop.record(plan, packet) };
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_d3d11_graphics(
    device: *mut *mut c_void,
    queue: *mut *mut c_void,
) -> bool {
    // The caller holds callback admission until it receives both owning references.
    let Some(shared) = lock().view() else {
        return false;
    };
    let owning_device = unsafe { borrowed::<ID3D12Device>(&shared.device12) }.clone();
    let owning_queue = unsafe { borrowed::<ID3D12CommandQueue>(&shared.queue12) }.clone();
    unsafe {
        device.write(owning_device.into_raw());
        queue.write(owning_queue.into_raw());
    }
    true
}
