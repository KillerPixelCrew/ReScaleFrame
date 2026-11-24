// SPDX-License-Identifier: MIT
//! A per-output D3D12 presenter. The caller owns window selection and leases the source.
use crate::d3d11_d3d12::transition_barrier;
use crate::sync::lock;
use rsf_game_sys::{RSF_GAME_RENDER_WINDOW, rsf_game_render_pass};
use rsf_runtime_submission::{FenceStatus, SubmissionRing, fence_status};
use std::ffi::c_void;
use std::sync::{Mutex, TryLockError};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::{
    Direct3D12::*,
    Dxgi::{Common::*, *},
};
use windows::core::Interface;
use windows_sys::Win32::{
    Foundation::{CloseHandle, S_OK},
    System::Threading::CreateEventW,
};

type Observer = Option<unsafe extern "C" fn(*const rsf_game_render_pass) -> i32>;

const SLOTS: usize = 3;
const FORMAT: DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM;

/// `rsf_d3d12_window_target`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Target {
    struct_size: u32,
    version: u32,
    handle: *mut c_void,
    generation: u64,
}
const _: () = {
    assert!(size_of::<Target>() == 24);
    assert!(std::mem::offset_of!(Target, generation) == 16);
};

impl Target {
    const EMPTY: Self = Self {
        struct_size: size_of::<Self>() as u32,
        version: 1,
        handle: std::ptr::null_mut(),
        generation: 0,
    };
}

/// The bound device, its direct queue and the presenter's fence, and the swapchain on the current
/// target. Fields drop in order: swapchain, fence, queue, device.
struct Gpu {
    chain: Option<IDXGISwapChain3>,
    fence: ID3D12Fence,
    queue: ID3D12CommandQueue,
    device: ID3D12Device,
}

/// One ring slot's allocator and command list, created on the slot's first use, and the source
/// texture its last submission copied from.
struct Slot {
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    source: Option<ID3D12Resource>,
}

enum Recorded {
    Closed,
    // The allocator or source refused; the slot stays usable.
    Refused,
    // The list could not reset or close; the slot is dropped.
    Broken,
}

struct Graphics {
    // Slots drop before the device objects they were created from.
    slots: [Option<Slot>; SLOTS],
    gpu: Option<Gpu>,
    /// A refused signal left a recording with the queue. Keep every reference and refuse reuse.
    faulted: bool,
    /// The last drained check found every submission complete, and nothing was executed since.
    proved: bool,
    ring: SubmissionRing,
    target: Target,
    replace_chain: bool,
}

/// Takes one reference each to the lent device and direct queue and creates the fence.
unsafe fn bind(device: *mut c_void, queue: *mut c_void) -> Option<Gpu> {
    let device = unsafe { ID3D12Device::from_raw_borrowed(&device) }?.clone();
    let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }?.clone();
    if unsafe { queue.GetDesc() }.Type != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return None;
    }
    let mut owner: Option<ID3D12Device> = None;
    if unsafe { queue.GetDevice(&mut owner) }.is_err() || owner.as_ref() != Some(&device) {
        return None;
    }
    let fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.ok()?;
    Some(Gpu {
        chain: None,
        fence,
        queue,
        device,
    })
}

fn create_chain(
    queue: &ID3D12CommandQueue,
    window: *mut c_void,
    width: u32,
    height: u32,
) -> Option<IDXGISwapChain3> {
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: FORMAT,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferCount: SLOTS as u32,
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        ..Default::default()
    };
    unsafe {
        CreateDXGIFactory1::<IDXGIFactory2>()
            .and_then(|factory| {
                factory.CreateSwapChainForHwnd(
                    queue,
                    HWND(window),
                    &desc,
                    None,
                    None::<&IDXGIOutput>,
                )
            })
            .and_then(|chain| chain.cast::<IDXGISwapChain3>())
    }
    .ok()
}

fn wait(fence: &ID3D12Fence, value: u64) -> bool {
    match fence_status(unsafe { fence.GetCompletedValue() }, value) {
        FenceStatus::Complete => return true,
        FenceStatus::Removed => return false,
        FenceStatus::Pending => {}
    }
    let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
    if event.is_null() {
        return false;
    }
    let result = unsafe { crate::d3d12_fence::wait_fence(fence.as_raw(), value, event, 10_000) };
    unsafe { CloseHandle(event) };
    result == S_OK
}

fn create_slot(device: &ID3D12Device) -> Option<Slot> {
    unsafe {
        let allocator: ID3D12CommandAllocator = device
            .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
            .ok()?;
        let list: ID3D12GraphicsCommandList = device
            .CreateCommandList(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &allocator,
                None::<&ID3D12PipelineState>,
            )
            .ok()?;
        list.Close().ok()?;
        Some(Slot {
            allocator,
            list,
            source: None,
        })
    }
}

fn record_copy(
    slot: &Slot,
    destination: &ID3D12Resource,
    source: &ID3D12Resource,
    common: bool,
) -> Recorded {
    let (mut source_device, mut destination_device) = (None::<ID3D12Device>, None::<ID3D12Device>);
    unsafe {
        if source == destination
            || source.GetDevice(&mut source_device).is_err()
            || destination.GetDevice(&mut destination_device).is_err()
            || source_device != destination_device
            || slot.allocator.Reset().is_err()
        {
            return Recorded::Refused;
        }
        let list = &slot.list;
        if list
            .Reset(&slot.allocator, None::<&ID3D12PipelineState>)
            .is_err()
        {
            return Recorded::Broken;
        }
        let state = if common {
            D3D12_RESOURCE_STATE_COMMON
        } else {
            D3D12_RESOURCE_STATE_RENDER_TARGET
        };
        list.ResourceBarrier(&[
            transition_barrier(source, state, D3D12_RESOURCE_STATE_COPY_SOURCE),
            transition_barrier(
                destination,
                D3D12_RESOURCE_STATE_PRESENT,
                D3D12_RESOURCE_STATE_COPY_DEST,
            ),
        ]);
        list.CopyResource(destination, source);
        list.ResourceBarrier(&[
            transition_barrier(
                destination,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_PRESENT,
            ),
            transition_barrier(source, D3D12_RESOURCE_STATE_COPY_SOURCE, state),
        ]);
        if list.Close().is_ok() {
            Recorded::Closed
        } else {
            Recorded::Broken
        }
    }
}

fn compatible(source: &ID3D12Resource, width: u32, height: u32) -> bool {
    let desc = unsafe { source.GetDesc() };
    desc.Width == u64::from(width)
        && desc.Height == height
        && desc.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
        && desc.DepthOrArraySize == 1
        && desc.MipLevels == 1
        && desc.SampleDesc.Count == 1
        && desc.SampleDesc.Quality == 0
        && matches!(
            desc.Format,
            DXGI_FORMAT_R8G8B8A8_UNORM
                | DXGI_FORMAT_R8G8B8A8_TYPELESS
                | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
        )
}

impl Graphics {
    const fn new() -> Self {
        Self {
            slots: [const { None }; SLOTS],
            gpu: None,
            faulted: false,
            proved: false,
            ring: SubmissionRing::new(SLOTS),
            target: Target::EMPTY,
            replace_chain: false,
        }
    }

    fn prepare(
        &mut self,
        window: Target,
        device: *mut c_void,
        queue: *mut c_void,
        width: u32,
        height: u32,
    ) -> bool {
        if self.faulted
            || window.handle.is_null()
            || window.generation < self.target.generation
            || (window.generation == self.target.generation && window.handle != self.target.handle)
            || device.is_null()
            || queue.is_null()
            || width == 0
            || height == 0
        {
            return false;
        }
        if self.gpu.is_none() {
            self.gpu = unsafe { bind(device, queue) };
        }
        let Some(gpu) = &self.gpu else {
            return false;
        };
        if gpu.device.as_raw() != device || gpu.queue.as_raw() != queue {
            return false;
        }
        if self.target != window {
            // Refuse presentations to the previous target immediately. Keep its native
            // chain until the queue proves the copies complete, then retry the new target.
            self.target = window;
            self.replace_chain = true;
        }
        let replace = self.replace_chain;
        if replace && !self.check_drained() {
            return false;
        }
        self.replace_chain = false;
        let submitted = self.ring.submitted();
        let gpu = self.gpu.as_mut().expect("bound above");
        if replace {
            gpu.chain = None;
        }
        let Some(chain) = &gpu.chain else {
            gpu.chain = create_chain(&gpu.queue, window.handle, width, height);
            return gpu.chain.is_some();
        };
        let Ok(desc) = (unsafe { chain.GetDesc1() }) else {
            return false;
        };
        (desc.Width, desc.Height) == (width, height)
            || (wait(&gpu.fence, submitted)
                && unsafe {
                    chain.ResizeBuffers(
                        SLOTS as u32,
                        width,
                        height,
                        FORMAT,
                        DXGI_SWAP_CHAIN_FLAG(0),
                    )
                }
                .is_ok())
    }

    /// Copies `texture` to the current back buffer through the next ring slot, once that slot's
    /// last submission completed, and executes and signals it.
    fn submit(&mut self, texture: &ID3D12Resource, common: bool) -> bool {
        if self.faulted || self.ring.next_completion().is_none() {
            return false;
        }
        let Some(gpu) = &self.gpu else {
            return false;
        };
        let (index, retired) = self.ring.slot();
        // Only a retired slot may drop its source reference and reset its allocator.
        if !wait(&gpu.fence, retired) {
            return false;
        }
        // Resolve the destination before touching the slot, so a refusal leaves no open list.
        let Some(backbuffer) = gpu.chain.as_ref().and_then(|chain| unsafe {
            chain
                .GetBuffer::<ID3D12Resource>(chain.GetCurrentBackBufferIndex())
                .ok()
        }) else {
            return false;
        };
        let Some(entry) = self.slots.get_mut(index) else {
            return false;
        };
        if let Some(slot) = entry.as_mut() {
            slot.source = None;
        } else {
            *entry = create_slot(&gpu.device);
        }
        let Some(slot) = entry.as_mut() else {
            return false;
        };
        match record_copy(slot, &backbuffer, texture, common) {
            Recorded::Closed => {}
            Recorded::Refused => return false,
            Recorded::Broken => {
                *entry = None;
                return false;
            }
        }
        // The slot holds its source until the slot's next submission proves this one complete.
        slot.source = Some(texture.clone());
        self.proved = false;
        unsafe { gpu.queue.ExecuteCommandLists(&[Some((*slot.list).clone())]) };
        // The GPU already owns the list and source. A missing or refused signal
        // leaves this slot current and bars all further use or retirement.
        if !self
            .ring
            .submit(index)
            .is_some_and(|complete| unsafe { gpu.queue.Signal(&gpu.fence, complete) }.is_ok())
        {
            self.faulted = true;
            return false;
        }
        self.ring.advance(index);
        true
    }

    fn present(
        &mut self,
        target: Target,
        texture: &ID3D12Resource,
        pass: &rsf_game_render_pass,
        common: bool,
        observer: Observer,
    ) -> bool {
        let Some(gpu) = self.gpu.as_ref().filter(|gpu| gpu.chain.is_some()) else {
            return false;
        };
        if self.faulted || self.replace_chain || self.target != target {
            return false;
        }
        let (width, height) = (pass.camera.output_width, pass.camera.output_height);
        let (device, queue) = (gpu.device.as_raw(), gpu.queue.as_raw());
        if !compatible(texture, width, height)
            || !self.prepare(target, device, queue, width, height)
            || !self.submit(texture, common)
        {
            return false;
        }
        let chain = self.gpu.as_ref().and_then(|gpu| gpu.chain.as_ref());
        let chain = chain.expect("prepared chain");
        let mut pass = *pass;
        pass.swapchain = chain.as_raw();
        pass.role = RSF_GAME_RENDER_WINDOW;
        if let Some(observe) = observer
            && unsafe { observe(&raw const pass) } == 0
        {
            return false;
        }
        unsafe { chain.Present(0, DXGI_PRESENT(0)) }.0 == S_OK
    }

    /// Reads the fence without waiting. A device-removal read of `u64::MAX` refuses, as does a
    /// fault. Without a fence the read is 0.
    fn check_drained(&mut self) -> bool {
        let completed = self
            .gpu
            .as_ref()
            .map_or(0, |gpu| unsafe { gpu.fence.GetCompletedValue() });
        self.proved = !self.faulted && self.ring.drained(completed);
        self.proved
    }
}

// These symbols are linked into the package's existing Runtime core. Entry points require
// caller admission to remain closed between the final proof and destruction.
struct Output(Mutex<Graphics>, Observer);

fn read_target(target: *const Target) -> Option<Target> {
    if target.is_null() || unsafe { target.cast::<u32>().read() } < size_of::<Target>() as u32 {
        return None;
    }
    let mut target = unsafe { target.read() };
    target.struct_size = size_of::<Target>() as u32;
    (target.version == 1 && target.generation != 0 && !target.handle.is_null()).then_some(target)
}

fn with_output(output: &Output, call: impl FnOnce(&mut Graphics) -> bool) -> i32 {
    let mut graphics = match output.0.try_lock() {
        Ok(graphics) => graphics,
        Err(TryLockError::Poisoned(error)) => error.into_inner(),
        Err(TryLockError::WouldBlock) => return 0,
    };
    i32::from(call(&mut graphics))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_d3d12_window_create(observer: Observer) -> *mut c_void {
    Box::into_raw(Box::new(Output(Mutex::new(Graphics::new()), observer))).cast()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d12_window_prepare(
    output: *mut c_void,
    target: *const Target,
    device: *mut c_void,
    queue: *mut c_void,
    width: u32,
    height: u32,
) -> i32 {
    let (Some(output), Some(target)) = (
        unsafe { output.cast::<Output>().as_ref() },
        read_target(target),
    ) else {
        return 0;
    };
    with_output(output, |graphics| {
        graphics.prepare(target, device, queue, width, height)
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d12_window_present(
    output: *mut c_void,
    target: *const Target,
    texture: *mut c_void,
    pass: *const rsf_game_render_pass,
    common: u32,
) -> i32 {
    let (Some(output), Some(target), Some(texture)) = (
        unsafe { output.cast::<Output>().as_ref() },
        read_target(target),
        unsafe { ID3D12Resource::from_raw_borrowed(&texture) },
    ) else {
        return 0;
    };
    if pass.is_null()
        || unsafe { pass.cast::<u32>().read() } < size_of::<rsf_game_render_pass>() as u32
    {
        return 0;
    }
    let pass = unsafe { &*pass };
    with_output(output, |graphics| {
        graphics.present(target, texture, pass, common != 0, output.1)
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d12_window_drained(output: *mut c_void) -> i32 {
    unsafe { output.cast::<Output>().as_ref() }
        .map_or(1, |output| with_output(output, Graphics::check_drained))
}

/// Destroys the record only with a drained proof taken since its last submission. A refusal
/// destroys nothing.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d12_window_destroy(output: *mut c_void) -> i32 {
    let output = output.cast::<Output>();
    let Some(output_ref) = (unsafe { output.as_ref() }) else {
        return 1;
    };
    if !lock(&output_ref.0).proved {
        return 0;
    }
    drop(unsafe { Box::from_raw(output) });
    1
}
