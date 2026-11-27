// SPDX-License-Identifier: MIT
//! The linked shared FG executable. Native D3D12 stands in for Unity and D3D11 for AC7; one run drives
//! a real chain through the provider callbacks or the D3D12 service, runtime switches and the overlay.
use crate::d3d11_device_checks::flip_source;
use crate::fg_fixture_focus::Focus;
use crate::present_fixture_startup::*;
use crate::present_fixture_time::{frequency, stamp};
use crate::sys::*;
use rsf_game_sys::backend_result::NOT_SUPPORTED;
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_SCREEN_FLIGHT, rsf_camera_frame, rsf_frame_record,
    rsf_game_render_pass,
};
use rsf_test_support::d3d12::{buffer, copy_location, transition, wait_for_signal};
use rsf_test_support::{Window, borrow, enable_d3d12_debug_layer, frame_record};
use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, c_char};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::thread::{self, JoinHandle};
use windows::Win32::Foundation::{CloseHandle, HANDLE, RECT, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D11::{ID3D11RenderTargetView, ID3D11Texture2D};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R32_FLOAT, DXGI_MODE_DESC,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{BOOL, Interface, w};

const PHASE_UI_COMPLETE: u32 = 5;
const SIMULATION_START: u32 = 0;
const SIMULATION_END: u32 = 1;
const RENDER_SUBMIT_START: u32 = 2;
const RENDER_SUBMIT_END: u32 = 3;
const PRESENT_START: u32 = 4;
const PRESENT_END: u32 = 5;

unsafe extern "C" {
    fn atexit(callback: extern "C" fn()) -> i32;
}

/// A command allocator and the list recorded through it.
type Commands = (ID3D12CommandAllocator, ID3D12GraphicsCommandList);

/// The depth and motion textures and the descriptor heaps the fixture clears them through.
struct Inputs {
    depth: ID3D12Resource,
    motion: ID3D12Resource,
    cpu_heap: ID3D12DescriptorHeap,
    gpu_heap: ID3D12DescriptorHeap,
    color_heap: Option<ID3D12DescriptorHeap>,
    stride: u32,
}

// What the provider callbacks write through `user`. Only the presentation thread enters them; the
// query worker only reads DXGI state and the window callback does not use this state.
struct State {
    frame: Cell<u64>,
    accepted: Cell<bool>,
    samples: Cell<u64>,
    captured_frame: Cell<u64>,
    readback: RefCell<Option<ID3D12Resource>>,
    inputs: RefCell<Option<Inputs>>,
    service: bool,
    hot: bool,
    flip: bool,
    mfg: bool,
}

impl State {
    fn check(&self, accepted: bool) {
        self.accepted.set(self.accepted.get() && accepted);
    }

    fn source(&self) -> u64 {
        source_id(self.frame.get(), self.hot)
    }
}

// The provider can retain callback inputs after a failed GPU retirement.
static PROCESS_STATE: AtomicPtr<State> = AtomicPtr::new(null_mut());

extern "C" fn release_state() {
    let state = PROCESS_STATE.swap(null_mut(), Ordering::AcqRel);
    if !state.is_null() {
        drop(unsafe { Box::from_raw(state) });
    }
}

/// The source id reported for `frame`: the frame itself, or with runtime switching a provider epoch
/// in the high word (a Unity-style session epoch change and ids above 32 bits).
fn source_id(frame: u64, switching: bool) -> u64 {
    if switching {
        ((if frame < 21 { 1 } else { 2 }) << 32) | frame
    } else {
        frame
    }
}

unsafe extern "C" fn log(_: Raw, message: *const c_char) {
    println!(
        "SDK: {}",
        unsafe { CStr::from_ptr(message) }.to_string_lossy()
    );
}

unsafe extern "C" fn accept(_: Raw, _: Raw) -> i32 {
    1
}

fn provider() -> Option<&'static rsf_generation_provider> {
    // Provider tables are immutable for the lifetime of the linked vendors.
    unsafe { rsf_d3d11_present_provider().as_ref() }
}

unsafe fn provider_status(api: &rsf_generation_provider, session: Raw) -> Option<rsf_fg_status> {
    let mut status = rsf_fg_status {
        struct_size: size_of::<rsf_fg_status>() as u32,
        ..Default::default()
    };
    (unsafe { (api.status?)(session, &mut status) } == 0).then_some(status)
}

fn capability_snapshot() -> rsf_native_fg_status {
    let mut status = rsf_native_fg_status {
        struct_size: size_of::<rsf_native_fg_status>() as u32,
        ..Default::default()
    };
    unsafe { rsf_fg12_status(&mut status) };
    status
}

fn resource(
    resource: &ID3D12Resource,
    width: u32,
    height: u32,
    state: D3D12_RESOURCE_STATES,
) -> rsf_backend_resource {
    rsf_backend_resource {
        struct_size: size_of::<rsf_backend_resource>() as u32,
        resource: resource.as_raw(),
        generation: 1,
        width,
        height,
        state: state.0 as u32,
        ..Default::default()
    }
}

fn resource_device(resource: &ID3D12Resource) -> Option<ID3D12Device> {
    let mut device = None;
    unsafe { resource.GetDevice(&mut device) }.ok()?;
    device
}

/// Clears depth and motion through the shader-visible heap and leaves them readable by the provider.
fn record_inputs(commands: &ID3D12GraphicsCommandList, inputs: &Inputs) {
    unsafe { commands.SetDescriptorHeaps(&[Some(inputs.gpu_heap.clone())]) };
    let mut cpu = unsafe { inputs.cpu_heap.GetCPUDescriptorHandleForHeapStart() };
    let mut gpu = unsafe { inputs.gpu_heap.GetGPUDescriptorHandleForHeapStart() };
    for texture in [&inputs.depth, &inputs.motion] {
        transition(
            commands,
            texture,
            D3D12_RESOURCE_STATE_COMMON,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        );
        unsafe {
            commands.ClearUnorderedAccessViewFloat(gpu, cpu, texture, &[0.5, 0.0, 0.0, 0.0], &[])
        };
        transition(
            commands,
            texture,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        );
        cpu.ptr += inputs.stride as usize;
        gpu.ptr += u64::from(inputs.stride);
    }
}

/// The frame record the provider is given for `backbuffer`, sized from `backbuffer` and `depth`.
fn frame(backbuffer: &ID3D12Resource, depth: &ID3D12Resource, source: u64) -> rsf_frame_record {
    let output = unsafe { backbuffer.GetDesc() };
    let render = unsafe { depth.GetDesc() };
    let mut record = rsf_frame_record {
        resource_generation: 1,
        input_qpc: stamp(),
        phase: PHASE_UI_COMPLETE,
        screen: RSF_SCREEN_FLIGHT,
        frame_time_ms: 16.67,
        ..frame_record(
            1,
            source,
            1,
            [render.Width as u32, render.Height],
            [output.Width as u32, output.Height],
        )
    };
    let camera = &mut record.camera;
    camera.struct_size = size_of::<rsf_camera_frame>() as u32;
    camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    camera.near_plane = 0.1;
    camera.far_plane = 1000.0;
    camera.vertical_fov_radians = 1.0;
    camera.depth_inverted = 1;
    for diagonal in [0, 5, 10, 15] {
        camera.view_to_clip[diagonal] = 1.0;
        camera.clip_to_view[diagonal] = 1.0;
        camera.view_to_world[diagonal] = 1.0;
        camera.world_to_view[diagonal] = 1.0;
        camera.clip_to_previous_clip[diagonal] = 1.0;
    }
    record
}

/// Clears `color` to the HUD-less scene (with moving tiles for the MFG fixture) through `heap`.
/// False when the resource has no device.
fn hudless(
    list: &ID3D12GraphicsCommandList,
    color: &ID3D12Resource,
    heap: &ID3D12DescriptorHeap,
    frame: u64,
    mfg: bool,
    width: u32,
    height: u32,
) -> bool {
    let Some(device) = resource_device(color) else {
        return false;
    };
    let target = unsafe { heap.GetCPUDescriptorHandleForHeapStart() };
    unsafe { device.CreateRenderTargetView(color, None, target) };
    transition(
        list,
        color,
        D3D12_RESOURCE_STATE_PRESENT,
        D3D12_RESOURCE_STATE_RENDER_TARGET,
    );
    unsafe { list.ClearRenderTargetView(target, &[0.2, 0.4, 0.6, 1.0], None) };
    if !mfg {
        return true;
    }
    // Solid identical frames can be classified as menus by DLSS-G; the tiles supply changing detail.
    let (width, height) = (width as i32, height as i32);
    let shift = ((frame * 3) % 64) as i32;
    for y in (0..height).step_by(64) {
        for x in (shift - 64..width).step_by(64) {
            let tile = RECT {
                left: x.max(0),
                top: y,
                right: width.min(x + 32),
                bottom: height.min(y + 32),
            };
            if tile.right > tile.left {
                let detail = [0.7, 0.25 + ((y / 64) % 3) as f32 * 0.15, 0.15, 1.0];
                unsafe { list.ClearRenderTargetView(target, &detail, Some(&[tile])) };
            }
        }
    }
    true
}

/// Copies the first texel of `back` into `readback`, creating the readback buffer on first use.
fn capture_flip(
    list: &ID3D12GraphicsCommandList,
    back: &ID3D12Resource,
    readback: &mut Option<ID3D12Resource>,
) -> bool {
    if readback.is_none() {
        *readback = resource_device(back)
            .and_then(|device| buffer(&device, 256, D3D12_HEAP_TYPE_READBACK).ok());
    }
    let Some(readback) = readback else {
        return false;
    };
    let source = copy_location(back, None);
    let target = copy_location(
        readback,
        Some(D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
            Offset: 0,
            Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                Width: 1,
                Height: 1,
                Depth: 1,
                RowPitch: 256,
            },
        }),
    );
    let pixel = D3D12_BOX {
        left: 0,
        top: 0,
        front: 0,
        right: 1,
        bottom: 1,
        back: 1,
    };
    transition(
        list,
        back,
        D3D12_RESOURCE_STATE_PRESENT,
        D3D12_RESOURCE_STATE_COPY_SOURCE,
    );
    unsafe { list.CopyTextureRegion(&target, 0, 0, 0, &source, Some(&pixel)) };
    transition(
        list,
        back,
        D3D12_RESOURCE_STATE_COPY_SOURCE,
        D3D12_RESOURCE_STATE_PRESENT,
    );
    true
}

/// Whether the first texel in `readback` is the colour `frame`'s clear should have produced.
/// `None` when it cannot be read.
fn check_flip(readback: &ID3D12Resource, frame: u64) -> Option<bool> {
    let mut pixel = null_mut();
    unsafe { readback.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 4 }), Some(&mut pixel)) }.ok()?;
    let rgba = (!pixel.is_null()).then(|| unsafe { pixel.cast::<[u8; 4]>().read() });
    unsafe { readback.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 })) };
    let [r, g, b, a] = rgba?;
    let expected = r == if !frame.is_multiple_of(2) { 255 } else { 0 };
    let matches = expected && (127..=128).contains(&g) && (63..=64).contains(&b) && a == 255;
    if !matches {
        eprintln!("FAIL: DX11 cached-view frame {frame} presented RGBA={r},{g},{b},{a}");
    }
    Some(matches)
}

/// Records the frame's inputs on `commands` and hands them to the provider, or captures them for
/// the service loop.
fn prepare_input(
    state: &State,
    commands: &ID3D12GraphicsCommandList,
    back: &ID3D12Resource,
    session: Raw,
) {
    if state.flip {
        state.captured_frame.set(state.frame.get());
        if !capture_flip(commands, back, &mut state.readback.borrow_mut()) {
            state.check(false);
            return;
        }
    }
    let api = provider();
    if !state.service && (api.is_none() || session.is_null()) {
        return;
    }
    let inputs = state.inputs.borrow();
    let Some(inputs) = inputs.as_ref() else {
        state.check(false);
        return;
    };
    if !state.service {
        let options = rsf_fg_options {
            struct_size: size_of::<rsf_fg_options>() as u32,
            abi_version: RSF_FG_ABI_VERSION,
            mode: RSF_FG_FIXED,
            generated_frames: 1,
            reflex_mode: RSF_REFLEX_OFF,
            ..Default::default()
        };
        state.check(
            api.and_then(|api| api.configure)
                .is_some_and(|configure| unsafe { configure(session, &options) } == 0),
        );
    }
    record_inputs(commands, inputs);
    let record = frame(back, &inputs.depth, state.source());
    let provider_frame = rsf_fg_frame {
        struct_size: size_of::<rsf_fg_frame>() as u32,
        record: &record,
        interpolate: 1,
        motion_scale_x: 1.0,
        motion_scale_y: 1.0,
        backbuffer: resource(
            back,
            record.output_width,
            record.output_height,
            D3D12_RESOURCE_STATE_PRESENT,
        ),
        depth: resource(
            &inputs.depth,
            record.render_width,
            record.render_height,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        motion: resource(
            &inputs.motion,
            record.render_width,
            record.render_height,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        ..Default::default()
    };
    if state.service {
        unsafe {
            rsf_fg12_capture(
                commands.as_raw(),
                &record,
                inputs.depth.as_raw(),
                inputs.motion.as_raw(),
            )
        };
    }
    let ready = !state.hot || {
        let heap = inputs.color_heap.as_ref().unwrap();
        let (frame, width, height) = (state.frame.get(), record.output_width, record.output_height);
        let ready = hudless(commands, back, heap, frame, state.mfg, width, height);
        if ready {
            let pass = rsf_game_render_pass {
                struct_size: size_of::<rsf_game_render_pass>() as u32,
                session_id: 1,
                source_frame_id: record.frame_id,
                view_key: 1,
                color_input: back.as_raw(),
                ..Default::default()
            };
            unsafe { rsf_fg12_hudless(commands.as_raw(), &pass) };
            transition(
                commands,
                back,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                D3D12_RESOURCE_STATE_PRESENT,
            );
        }
        ready
    };
    if !ready {
        state.check(false);
    }
    if ready && !state.service {
        let result = api
            .and_then(|api| api.prepare)
            .map_or(-1, |prepare| unsafe {
                prepare(session, commands.as_raw(), &provider_frame)
            });
        if result != 0 {
            println!("prepare frame={} refused={result}", state.frame.get());
        }
        state.check(result == 0);
    }
    for texture in [&inputs.depth, &inputs.motion] {
        transition(
            commands,
            texture,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
            D3D12_RESOURCE_STATE_COMMON,
        );
    }
}

/// The presentation setup's prepare callback; `user` is the fixture [`State`].
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn prepare_callback(
    user: Raw,
    _: Raw,
    _: Raw,
    commands: Raw,
    back: Raw,
    _: *mut rsf_streamline_host,
    session: Raw,
    _: u32,
) {
    let state = unsafe { &*user.cast::<State>() };
    unsafe { prepare_input(state, borrow(&commands), borrow(&back), session) };
}

unsafe extern "C" fn latency_callback(user: Raw, event: *const rsf_observer_present_event) {
    let Some(event) = (unsafe { event.as_ref() }) else {
        return;
    };
    let state = unsafe { &*user.cast::<State>() };
    let session = unsafe { rsf_d3d11_present_session() };
    let Some(mark) = provider().and_then(|api| api.marker) else {
        return;
    };
    if session.is_null() {
        return;
    }
    if event.completed == 0 {
        state.check(unsafe { mark(session, RENDER_SUBMIT_END, state.source(), 0) } == 0);
        state.check(unsafe { mark(session, PRESENT_START, state.source(), 0) } == 0);
    } else {
        state.check(unsafe { mark(session, PRESENT_END, state.source(), 0) } == 0);
    }
}

unsafe extern "C" fn retire_callback(user: Raw, session: Raw) {
    let state = unsafe { &*user.cast::<State>() };
    let Some(api) = provider() else {
        return;
    };
    if session.is_null() {
        return;
    }
    let status = unsafe { provider_status(api, session) };
    state.check(status.is_some());
    let active = status.map_or(0, |status| status.active);
    state.samples.set(state.samples.get() + u64::from(active));
    let (mut fence, mut value) = (null_mut(), 0);
    let result = unsafe { rsf_d3d11_present_retire(session, null_mut(), &mut fence, &mut value) };
    state.check(result == 0 && !fence.is_null() && value != 0);
}

// Engine worker threads query the chain throughout; the caller's COM lease outlives this worker on
// every exit, including an early refusal.
struct Queries {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<(u32, u32)>>,
}

impl Queries {
    fn start(chain: &IDXGISwapChain) -> Option<Self> {
        let receiver = chain.as_raw() as usize;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::Builder::new()
            .spawn(move || {
                let raw = receiver as Raw;
                let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&raw) }.unwrap();
                let (mut count, mut failed, mut previous) = (0, 0, 0);
                while !stopping.load(Ordering::Acquire) {
                    let mut fullscreen = BOOL(0);
                    if unsafe { chain.GetFullscreenState(Some(&mut fullscreen), None) }
                        .is_err_and(|error| error.code() != DXGI_ERROR_WAS_STILL_DRAWING)
                    {
                        failed += 1;
                    }
                    if let Ok(presents) = unsafe { chain.GetLastPresentCount() } {
                        failed += u32::from(presents < previous);
                        previous = presents;
                    }
                    let _ =
                        unsafe { chain.GetFrameStatistics(&mut DXGI_FRAME_STATISTICS::default()) };
                    count += 1;
                    thread::yield_now();
                }
                (count, failed)
            })
            .ok()?;
        Some(Self {
            stop,
            worker: Some(worker),
        })
    }

    fn finish(&mut self) -> Option<(u32, u32)> {
        self.stop.store(true, Ordering::Release);
        self.worker.take()?.join().ok()
    }
}

impl Drop for Queries {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn allocate_inputs(device: &ID3D12Device, mixed: bool, hot: bool) -> windows::core::Result<Inputs> {
    let mut desc = D3D12_DESCRIPTOR_HEAP_DESC {
        Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
        NumDescriptors: 2,
        ..Default::default()
    };
    let cpu_heap: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap(&desc) }?;
    desc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    let gpu_heap: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap(&desc) }?;
    let stride = unsafe { device.GetDescriptorHandleIncrementSize(desc.Type) };
    let color_heap = if hot {
        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            NumDescriptors: 1,
            ..Default::default()
        };
        Some(unsafe { device.CreateDescriptorHeap(&desc) }?)
    } else {
        None
    };
    let mut cpu_slot = unsafe { cpu_heap.GetCPUDescriptorHandleForHeapStart() };
    let mut visible_slot = unsafe { gpu_heap.GetCPUDescriptorHandleForHeapStart() };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let mut resources = [None, None];
    for (slot, format) in resources
        .iter_mut()
        .zip([DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R16G16_FLOAT])
    {
        let texture = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: if mixed { 1485 } else { 1280 },
            Height: if mixed { 928 } else { 720 },
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            ..Default::default()
        };
        unsafe {
            device.CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &texture,
                D3D12_RESOURCE_STATE_COMMON,
                None,
                slot,
            )?;
            device.CreateUnorderedAccessView(
                slot.as_ref(),
                None::<&ID3D12Resource>,
                None,
                cpu_slot,
            );
            device.CopyDescriptorsSimple(1, visible_slot, cpu_slot, desc.Type);
        }
        cpu_slot.ptr += stride as usize;
        visible_slot.ptr += stride as usize;
    }
    let [Some(depth), Some(motion)] = resources else {
        return Err(windows::Win32::Foundation::E_POINTER.into());
    };
    Ok(Inputs {
        depth,
        motion,
        cpu_heap,
        gpu_heap,
        color_heap,
        stride,
    })
}

/// A closed direct command list and its allocator.
fn commands(device: &ID3D12Device) -> Result<Commands, i32> {
    let allocator: ID3D12CommandAllocator =
        unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.map_err(|_| 1)?;
    let list: ID3D12GraphicsCommandList = unsafe {
        device.CreateCommandList(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            &allocator,
            None::<&ID3D12PipelineState>,
        )
    }
    .map_err(|_| 1)?;
    unsafe { list.Close() }.map_err(|_| 1)?;
    Ok((allocator, list))
}

fn reset((allocator, list): &Commands) -> Result<(), i32> {
    unsafe { allocator.Reset() }
        .and_then(|()| unsafe { list.Reset(allocator, None::<&ID3D12PipelineState>) })
        .map_err(|_| 1)
}

fn execute(queue: &ID3D12CommandQueue, list: &ID3D12GraphicsCommandList) -> Result<(), i32> {
    unsafe { list.Close() }.map_err(|_| 1)?;
    let list = list.cast::<ID3D12CommandList>().map_err(|_| 1)?;
    unsafe { queue.ExecuteCommandLists(&[Some(list)]) };
    Ok(())
}

unsafe fn run(argv: &[*const c_char]) -> Result<i32, i32> {
    if !matches!(argv.len(), 5..=7) {
        println!(
            "SKIP: backend, SDK directory, vendor ID, API(11/12/12-hot), optional XeSS and Streamline directories"
        );
        return Ok(77);
    }
    let (backend, vendor) = unsafe { (number(argv[1]), number(argv[3])) };
    let debug = env_present(c"RSF_UNITY_GPU_DEBUG");
    if debug && !enable_d3d12_debug_layer() {
        return Ok(77);
    }
    let api = unsafe { CStr::from_ptr(argv[4]) }.to_bytes();
    let overlay = api == b"12-overlay";
    let mixed = api == b"12-mixed" || overlay;
    let mfg = api == b"12-mfg";
    let hot = api == b"12-hot" || mixed || mfg;
    let flip = api == b"11-flip";
    let legacy_hot = api == b"11-hot" || flip;
    let service = hot || api == b"12-service";
    let native = api != b"11" && !legacy_hot;
    let explicit = env_present(c"RSF_EXPLICIT_SWAPCHAIN");
    if explicit && !native {
        return Err(1);
    }
    // argv[5] is the XeSS directory and argv[6] the Streamline one; both fall back to argv[2].
    let primary = argv[2];
    let xess = argv.get(5).copied().unwrap_or(primary);
    let extra = argv.get(6).copied();
    let legacy_fsr = extra.unwrap_or(primary);
    let (factory, adapter) = adapter::<IDXGIFactory4>(vendor)?;
    let focus_test = mfg && env_present(c"RSF_MFG_FOCUS_TEST");
    let window = Window::new(
        w!("RSFSharedFG"),
        w!("Shared generation fixture"),
        WS_OVERLAPPEDWINDOW,
        1280,
        720,
    )
    .map_err(|_| 1)?;
    let hwnd = window.handle;
    // Declared before the graphics objects so the lease outlives them while the run unwinds.
    let focus: Option<Focus>;
    let state = Box::into_raw(Box::new(State {
        frame: Cell::new(0),
        accepted: Cell::new(true),
        samples: Cell::new(0),
        captured_frame: Cell::new(0),
        readback: RefCell::new(None),
        inputs: RefCell::new(None),
        service,
        hot,
        flip,
        mfg,
    }));
    PROCESS_STATE.store(state, Ordering::Release);
    // On CRT registration failure, the resource lease still lives until process exit.
    let _ = unsafe { atexit(release_state) };

    let (engine_type, engine_version, project_id) = if native {
        (2, c"6000.3", c"57a42c7e-faf0-4bda-a9f9-892870948ac1")
    } else {
        (1, c"4.18.3", c"a3ed1f08-3542-4698-b85c-e1a9908e861a")
    };
    let max_generated_frames = if mfg { 5 } else { 3 };
    let installed = if service {
        let mut setup = rsf_fg12_setup {
            struct_size: size_of::<rsf_fg12_setup>() as u32,
            backend,
            max_generated_frames,
            runtime_directory_utf8: primary,
            options: rsf_native_fg_options {
                struct_size: size_of::<rsf_native_fg_options>() as u32,
                mode: RSF_FG_FIXED,
                generated_frames: 1,
                reflex_mode: RSF_REFLEX_OFF,
                ..Default::default()
            },
            depth_inverted: 1,
            units_to_meters: 1.0,
            log: Some(log),
            accept_window: Some(accept),
            engine_type,
            engine_version_utf8: engine_version.as_ptr(),
            project_id_utf8: project_id.as_ptr(),
            ..Default::default()
        };
        if hot {
            setup.runtime_switching = 1;
            setup.ui_mode = RSF_UI_MODE_BACKBUFFER_HUDLESS;
            setup.fsr3_directory_utf8 = primary;
            setup.fsr4_directory_utf8 = primary;
            setup.xess_directory_utf8 = xess;
            setup.streamline_directory_utf8 = extra.unwrap_or(null());
        }
        let install = if explicit {
            rsf_fg12_install_explicit
        } else {
            rsf_fg12_install
        };
        unsafe { install(&setup) == 0 }
    } else {
        let mut setup = rsf_d3d11_present_setup {
            struct_size: size_of::<rsf_d3d11_present_setup>() as u32,
            runtime_directory_utf8: primary,
            backend,
            max_generated_frames,
            log: Some(log),
            user: state.cast(),
            accept_window: Some(accept),
            prepare: Some(prepare_callback),
            latency_event: Some(latency_callback),
            retire: Some(retire_callback),
            depth_inverted: 1,
            depth_infinite: u32::from(!native),
            units_to_meters: if native { 1.0 } else { 0.01 },
            engine_type,
            engine_version_utf8: engine_version.as_ptr(),
            project_id_utf8: project_id.as_ptr(),
            ..Default::default()
        };
        if legacy_hot {
            setup.runtime_switching = 1;
            setup.streamline_directory_utf8 = primary;
            setup.fsr3_directory_utf8 = legacy_fsr;
            setup.fsr4_directory_utf8 = legacy_fsr;
            setup.xess_directory_utf8 = xess;
        }
        let install = if explicit {
            rsf_d3d11_present_prepare
        } else {
            rsf_d3d11_present_install
        };
        unsafe { install(&setup) != 0 }
    };
    if !installed {
        return Err(1);
    }

    let (dx11, context, _dx12, queue) = if native {
        let (device, direct) = device12(&adapter)?;
        (None, None, Some(device), Some(direct))
    } else {
        let (device, immediate) = device11(&adapter)?;
        (Some(device), Some(immediate), None, None)
    };
    let producer = match (&queue, &dx11) {
        (Some(queue), _) => queue.as_raw(),
        (None, device) => device.as_ref().unwrap().as_raw(),
    };
    let flip_model = native || flip;
    let desc = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Width: if mixed { 2560 } else { 1280 },
            Height: if mixed { 1600 } else { 720 },
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ..Default::default()
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: if flip_model { 3 } else { 1 },
        OutputWindow: hwnd,
        Windowed: BOOL(1),
        SwapEffect: if flip_model {
            DXGI_SWAP_EFFECT_FLIP_DISCARD
        } else {
            DXGI_SWAP_EFFECT_DISCARD
        },
        Flags: if hot && native {
            DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32
        } else {
            0
        },
    };
    let chain: IDXGISwapChain = if explicit {
        let explicit_desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: desc.BufferDesc.Width,
            Height: desc.BufferDesc.Height,
            Format: desc.BufferDesc.Format,
            SampleDesc: desc.SampleDesc,
            BufferUsage: desc.BufferUsage,
            BufferCount: desc.BufferCount,
            SwapEffect: desc.SwapEffect,
            Flags: desc.Flags,
            ..Default::default()
        };
        let mut created = null_mut();
        let result = unsafe {
            rsf_d3d11_present_create_for_hwnd(
                factory.as_raw(),
                producer,
                hwnd.0,
                (&raw const explicit_desc).cast(),
                &mut created,
            )
        };
        if result < 0 || created.is_null() {
            return Err(1);
        }
        unsafe { IDXGISwapChain1::from_raw(created) }
            .cast()
            .map_err(|_| 1)?
    } else {
        create_chain(&factory, producer, &desc)?
    };
    if unsafe { rsf_d3d11_present_has_owner() } == 0 {
        println!("SKIP: provider refused, original engine chain retained");
        return Ok(77);
    }
    // The first engine query prepares the deferred provider outside the factory callback.
    let latency_chain = chain.cast::<IDXGISwapChain2>().map_err(|_| 1)?;
    unsafe { latency_chain.GetDesc1() }.map_err(|_| 1)?;
    let mut cached_rtv: Option<ID3D11RenderTargetView> = None;
    if flip {
        let back = unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }.map_err(|_| 1)?;
        unsafe {
            dx11.as_ref()
                .unwrap()
                .CreateRenderTargetView(&back, None, Some(&mut cached_rtv))
        }
        .map_err(|_| 1)?;
    }
    let flip_objects = match (&dx11, &context, &cached_rtv) {
        (Some(device), Some(context), Some(view)) => Some((device, context, view)),
        _ => None,
    };
    let application_wait = if hot && native {
        unsafe { latency_chain.SetMaximumFrameLatency(1) }.map_err(|_| 1)?;
        let handle = unsafe { latency_chain.GetFrameLatencyWaitableObject() };
        if handle.0.is_null() {
            return Err(1);
        }
        handle
    } else {
        HANDLE::default()
    };
    if backend == RSF_FG_BACKEND_AUTO {
        let expected = unsafe { rsf_fg_default_backend(vendor, 0) };
        if unsafe { rsf_d3d11_present_backend() } != expected
            || (expected != 0 && unsafe { rsf_d3d11_present_session() }.is_null())
        {
            println!("FAIL: Auto must create the resolved provider");
            return Err(1);
        }
        println!("PASS: Auto instantiated provider {expected} before the first Present");
    }
    let mut graphics = rsf_streamline_graphics {
        struct_size: size_of::<rsf_streamline_graphics>() as u32,
        ..Default::default()
    };
    if unsafe { rsf_d3d11_present_graphics(&mut graphics) } == 0 {
        return Err(1);
    }
    let device = graphics.native_device;
    let gpu = unsafe { ID3D12Device::from_raw_borrowed(&device) }.ok_or(1)?;
    let fsr4_directory = if legacy_hot { legacy_fsr } else { primary };
    let fsr4_supported = unsafe { rsf_generation_fsr4_supported(device, fsr4_directory) } != 0;
    let mut accepted =
        (unsafe { rsf_d3d11_present_backend_choices() } & (1 << RSF_FG_BACKEND_FSR4) != 0)
            == fsr4_supported;
    let mut dlss_sr = null_mut();
    if let (true, Some(directory)) = (hot, extra) {
        let mut path = unsafe { CStr::from_ptr(directory) }.to_bytes().to_vec();
        path.extend_from_slice(b"\\sl.interposer.dll");
        let interposer = CString::new(path).map_err(|_| 1)?;
        let mut sr = rsf_sr12_setup {
            struct_size: size_of::<rsf_sr12_setup>() as u32,
            abi_version: 1,
            device,
            backend: 1,
            output_width: 1280,
            output_height: 720,
            inverted_depth: 1,
            streamline_host: unsafe { rsf_d3d11_present_host() }.cast(),
            ..Default::default()
        };
        sr.dlss.struct_size = size_of::<rsf_dlss_setup>() as u32;
        sr.dlss.abi_version = RSF_DLSS_ABI_VERSION;
        sr.dlss.interposer_path_utf8 = interposer.as_ptr();
        sr.dlss.plugin_directory_utf8 = directory;
        sr.dlss.engine = 2; // RSF_DLSS_ENGINE_UNITY.
        sr.dlss.engine_version_utf8 = c"6000.3".as_ptr();
        sr.dlss.project_id_utf8 = c"57a42c7e-faf0-4bda-a9f9-892870948ac1".as_ptr();
        sr.dlss.require_signature = 1;
        sr.dlss.log = Some(log);
        if unsafe { rsf_sr12_create(&sr, &mut dlss_sr) } != 0 {
            return Err(1);
        }
        println!("PASS: native DLSS SR registered before runtime FSR/XeSS switches");
    }
    let state = unsafe { &*state };
    *state.inputs.borrow_mut() = Some(allocate_inputs(gpu, mixed, hot).map_err(|_| 1)?);
    let fence =
        unsafe { gpu.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) }.map_err(|_| 1)?;
    let source_commands = if service { Some(commands(gpu)?) } else { None };
    let finish_commands = if overlay { Some(commands(gpu)?) } else { None };
    let graphics_queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&graphics.queue) };
    focus = if focus_test {
        Some(Focus::start(hwnd).ok_or(1)?)
    } else {
        None
    };
    // Keep exactly the same engine buffer across Off, FSR, XeSS and DLSS replacement.
    let held = if hot {
        Some(unsafe { chain.GetBuffer::<ID3D12Resource>(0) }.map_err(|_| 1)?)
    } else {
        None
    };
    let mut queries = if hot {
        Some(Queries::start(&chain).ok_or(1)?)
    } else {
        None
    };
    let initial_api = provider();
    let mut session = unsafe { rsf_d3d11_present_session() };
    let (mut expected_tagged, mut samples, mut previous_total) = (0u64, 0u64, 0u64);
    let (mut mfg_active, mut active_fsr, mut active_xess) = ([0u32; 6], 0, 0);
    for id in 1..=if mfg { 130 } else { 40 } {
        state.frame.set(id);
        if focus.as_ref().is_some_and(|focus| !focus.valid()) {
            println!("FAIL: focus test ended at its time limit or after a human focus change");
            accepted = false;
            break;
        }
        if mfg {
            let mut message = MSG::default();
            while unsafe { PeekMessageW(&mut message, Some(hwnd), 0, 0, PM_REMOVE) }.as_bool() {
                unsafe { DispatchMessageW(&message) };
            }
            let generated = match id {
                45 => 2,
                60 | 100 => 3,
                75 => 5,
                90 => 1,
                _ => 0,
            };
            if generated != 0 {
                let changed = rsf_native_fg_options {
                    struct_size: size_of::<rsf_native_fg_options>() as u32,
                    mode: if id == 90 { RSF_FG_OFF } else { RSF_FG_FIXED },
                    generated_frames: generated,
                    reflex_mode: RSF_REFLEX_ON,
                    ..Default::default()
                };
                unsafe { rsf_fg12_options(&changed) };
            }
        }
        if !application_wait.0.is_null()
            && unsafe { WaitForSingleObject(application_wait, 10000) } != WAIT_OBJECT_0
        {
            println!("FAIL: retained application latency handle stalled at frame {id}");
            return Err(1);
        }
        let current = unsafe { rsf_d3d11_present_backend() };
        let epoch = unsafe { rsf_d3d11_present_generation() };
        session = unsafe { rsf_d3d11_present_session() };
        if service {
            let source = source_id(id, hot);
            for stage in [
                RSF_GAME_CPU_FRAME_BEGIN,
                RSF_GAME_CPU_PACING,
                RSF_GAME_CPU_INPUT_SAMPLE,
                RSF_GAME_CPU_SIMULATION_BEGIN,
                RSF_GAME_CPU_SIMULATION_END,
            ] {
                let event = rsf_game_cpu_event {
                    render_expected: 0,
                    ..rsf_test_support::cpu_event(1, source, stage, stamp(), frequency())
                };
                unsafe { rsf_fg12_cpu(&event) };
            }
            let (Some(queue), Some(source_commands)) = (&queue, &source_commands) else {
                return Err(1);
            };
            let chain3 = chain.cast::<IDXGISwapChain3>().map_err(|_| 1)?;
            let back =
                unsafe { chain3.GetBuffer::<ID3D12Resource>(chain3.GetCurrentBackBufferIndex()) }
                    .map_err(|_| 1)?;
            reset(source_commands)?;
            let list = &source_commands.1;
            // An input refusal must not leak the shared CPU token and stall later frames.
            if !hot || id != 7 {
                prepare_input(state, list, &back, session);
            }
            if overlay {
                transition(
                    list,
                    &back,
                    D3D12_RESOURCE_STATE_PRESENT,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                );
            }
            execute(queue, list)?;
            if hot || id != 11 {
                unsafe { rsf_fg12_submitted(1, source) };
            }
            let (receiver, view) = (
                chain.as_raw() as usize,
                if !hot && id == 10 { 2 } else { 1 },
            );
            // The render thread reports the window while the presentation thread waits on it.
            let callback = thread::Builder::new().spawn(move || {
                let pass = rsf_game_render_pass {
                    struct_size: size_of::<rsf_game_render_pass>() as u32,
                    session_id: 1,
                    native_frame: source,
                    source_frame_id: source,
                    view_key: view,
                    swapchain: receiver as Raw,
                    ..Default::default()
                };
                unsafe { rsf_fg12_window(&pass) };
            });
            callback.map_err(|_| 1)?.join().map_err(|_| 1)?;
            if let Some(finish_commands) = &finish_commands {
                match id {
                    2 | 16 => unsafe { rsf_overlay_input_set_visible(1) },
                    8 | 25 => unsafe { rsf_overlay_input_set_visible(0) },
                    _ => {}
                }
                let stats = rsf_overlay_stats {
                    struct_size: size_of::<rsf_overlay_stats>() as u32,
                    backend_name: c"Fixture".as_ptr(),
                    show_performance_hud: 1,
                    output_width: 2560,
                    output_height: 1600,
                    ..Default::default()
                };
                let mut intent = rsf_overlay_intent {
                    struct_size: size_of::<rsf_overlay_intent>() as u32,
                    ..Default::default()
                };
                accepted &= unsafe {
                    rsf_overlay_d3d12_frame(
                        gpu.as_raw(),
                        queue.as_raw(),
                        chain.as_raw(),
                        &stats,
                        &mut intent,
                        Some(log),
                        null_mut(),
                    )
                } != 0;
                reset(finish_commands)?;
                let list = &finish_commands.1;
                transition(
                    list,
                    &back,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PRESENT,
                );
                execute(queue, list)?;
            }
        } else {
            accepted &= if legacy_hot {
                let acquired = unsafe { rsf_d3d11_present_acquire(id) };
                let begun = unsafe { rsf_d3d11_present_begin(id, null_mut()) };
                acquired == 0 && begun == 0
            } else {
                initial_api
                    .and_then(|api| api.begin_frame)
                    .is_some_and(|begin| unsafe { begin(session, id) } == 0)
            };
            for marker in [SIMULATION_START, SIMULATION_END, RENDER_SUBMIT_START] {
                let result = if legacy_hot {
                    unsafe {
                        rsf_d3d11_present_marker(rsf_d3d11_present_generation(), id, marker, 0)
                    }
                } else {
                    initial_api
                        .and_then(|api| api.marker)
                        .map_or(-1, |mark| unsafe { mark(session, marker, id, 0) })
                };
                accepted &= result == 0;
            }
        }
        if let Some((_, context, view)) = flip_objects {
            // DX11 keeps this view across Present; it does not select a DX12-style index.
            let color = [(id % 2) as f32, 0.5, 0.25, 1.0];
            unsafe { context.ClearRenderTargetView(view, &color) };
        }
        let presented = unsafe { chain.Present(0, DXGI_PRESENT(0)) };
        if presented.is_err() {
            let reason =
                unsafe { gpu.GetDeviceRemovedReason() }.map_or_else(|error| error.code().0, |()| 0);
            println!(
                "Present failed frame={id} backend={} HRESULT={:08x} device={:08x}",
                unsafe { rsf_d3d11_present_backend() },
                presented.0 as u32,
                reason as u32
            );
            return Err(1);
        }
        if legacy_hot {
            unsafe { rsf_d3d11_present_abort(id) };
            if let Some(request) = match id {
                8 => Some(3),
                15 => Some(5),
                25 => Some(0),
                32 => Some(1),
                _ => None,
            } {
                accepted &= unsafe { rsf_d3d11_present_request(request) } == 0;
            }
            if let Some(expected) = match id {
                9 => Some(3),
                16 => Some(5),
                26 => Some(0),
                33 => Some(1),
                _ => None,
            } {
                let active = unsafe { rsf_d3d11_present_backend() };
                println!(
                    "legacy runtime switch frame={id} backend={active} result={}",
                    unsafe { rsf_d3d11_present_switch_result() }
                );
                accepted &= active == expected;
            }
        }
        if service {
            let result = capability_snapshot();
            let current_session = unsafe { rsf_d3d11_present_session() };
            if !current_session.is_null()
                && let Some(api) = provider()
                && let Some(capability) = unsafe { provider_status(api, current_session) }
            {
                let deferred = result.vendor.supported == 0
                    && result.vendor.max_generated_frames == 0
                    && epoch != unsafe { rsf_d3d11_present_generation() };
                let same = deferred
                    || (result.vendor.max_generated_frames == capability.max_generated_frames
                        && result.vendor.supported == capability.supported);
                if deferred {
                    println!(
                        "Capability report deferred after provider switch frame={id} backend={}",
                        unsafe { rsf_d3d11_present_backend() }
                    );
                }
                if !same {
                    println!(
                        "Capability mismatch frame={id} backend={} cached={}/{} current={}/{}",
                        unsafe { rsf_d3d11_present_backend() },
                        result.vendor.supported,
                        result.vendor.max_generated_frames,
                        capability.supported,
                        capability.max_generated_frames
                    );
                }
                accepted &= same;
            }
            let tagged = if hot {
                current != 0 && id != 7
            } else {
                id != 10 && id != 11
            };
            expected_tagged += u64::from(tagged);
            samples += u64::from(result.vendor.active);
            accepted &= result.last_result == 0 && result.tagged_frames == expected_tagged;
            if mfg && current == RSF_FG_BACKEND_DLSS {
                let vendor = &result.vendor;
                let delta = vendor.total_presented.saturating_sub(previous_total);
                if vendor.active != 0
                    && delta > u64::from(vendor.effective_generated_frames)
                    && let Some(count) =
                        mfg_active.get_mut(vendor.effective_generated_frames as usize)
                {
                    *count += 1;
                }
                previous_total = vendor.total_presented;
                accepted &= vendor.mfg_unlock_status == RSF_MFG_UNLOCK_READY
                    && vendor.max_generated_frames >= 5;
                println!(
                    "MFG frame={id} admission={} max={} configured={} active={} presented_delta={delta}",
                    vendor.mfg_unlock_status,
                    vendor.max_generated_frames,
                    vendor.effective_generated_frames,
                    vendor.active
                );
            }
            if !hot && matches!(id, 10 | 11) {
                accepted &= result.reason != 0;
            }
            if let Some(held) = &held {
                match current {
                    RSF_FG_BACKEND_FSR3 => active_fsr += result.vendor.active,
                    RSF_FG_BACKEND_XESS => active_xess += result.vendor.active,
                    _ => {}
                }
                accepted &= unsafe { chain.GetBuffer::<ID3D12Resource>(0) }
                    .is_ok_and(|buffer| buffer.as_raw() == held.as_raw());
                if id == 35 && vendor == 0x10de {
                    accepted &= unsafe { rsf_d3d11_present_backend() } == RSF_FG_BACKEND_FSR3
                        && unsafe { rsf_d3d11_present_switch_result() } == NOT_SUPPORTED;
                }
                if let Some(request) = match id {
                    2 if current == 0 => Some(3),
                    8 => Some(0),
                    15 => Some(5),
                    25 => Some(3),
                    34 if vendor == 0x10de => Some(4),
                    35 if extra.is_some() => Some(1),
                    110 if mfg => Some(0),
                    114 if mfg => Some(1),
                    _ => None,
                } {
                    let expected = if request == RSF_FG_BACKEND_FSR4 && !fsr4_supported {
                        NOT_SUPPORTED
                    } else {
                        0
                    };
                    accepted &= unsafe { rsf_d3d11_present_request(request) } == expected;
                }
                if id == 36 && extra.is_some() {
                    accepted &= unsafe { rsf_d3d11_present_backend() } == RSF_FG_BACKEND_DLSS;
                }
            }
        }
        let present_queue;
        let drain_queue = if flip {
            present_queue = unsafe { rsf_d3d11_present_queue() };
            unsafe { ID3D12CommandQueue::from_raw_borrowed(&present_queue) }
        } else {
            graphics_queue
        };
        if !drain_queue.is_some_and(|queue| wait_for_signal(queue, &fence, id, 10_000)) {
            return Err(1);
        }
        if let Some((device, context, view)) = flip_objects {
            let flipped = state
                .readback
                .borrow()
                .as_ref()
                .and_then(|readback| check_flip(readback, id));
            if flipped != Some(true) {
                if flipped == Some(false) {
                    flip_source(device, context, view, state.captured_frame.get());
                }
                return Err(1);
            }
        }
    }
    if mfg {
        let [_, x2, x3, x4, _, x6] = mfg_active;
        println!("MFG confirmed samples 2x={x2} 3x={x3} 4x={x4} 6x={x6}");
        accepted &= [x2, x3, x4, x6].into_iter().all(|count| count != 0);
    }
    if let Some(queries) = &mut queries {
        let (count, failures) = queries.finish().ok_or(1)?;
        println!("concurrent engine queries={count} failures={failures}");
        accepted &= count != 0 && failures == 0;
    }
    let status = if hot {
        let status = capability_snapshot().vendor;
        println!(
            "runtime switches: FSR active={active_fsr} XeSS active={active_xess} buffer identity preserved={}",
            u32::from(accepted)
        );
        accepted &= active_fsr != 0 && active_xess != 0;
        status
    } else {
        if legacy_hot {
            session = unsafe { rsf_d3d11_present_session() };
        }
        let api = if legacy_hot { provider() } else { initial_api };
        api.and_then(|api| unsafe { provider_status(api, session) })
            .unwrap_or_default()
    };
    accepted &= state.accepted.get();
    let samples = samples + state.samples.get();
    let version = CStr::from_bytes_until_nul(unsafe {
        std::slice::from_raw_parts(
            status.version_name.as_ptr().cast(),
            status.version_name.len(),
        )
    })
    .unwrap_or(c"")
    .to_string_lossy();
    println!(
        "backend={backend} API={} version={version} accepted={} active_samples={samples} callbacks={} total_presented={}",
        if native { 12 } else { 11 },
        u32::from(accepted),
        status.generated_callbacks,
        status.total_presented
    );
    let passed = accepted && (samples != 0 || legacy_hot);
    if passed && flip {
        println!("PASS: DX11 cached buffer-zero view reached every physical presentation");
    }
    if !application_wait.0.is_null() {
        let _ = unsafe { CloseHandle(application_wait) };
        if passed {
            println!("PASS: one application latency handle survived provider transitions");
        }
    }
    unsafe { rsf_sr12_destroy(dlss_sr) };
    if overlay {
        unsafe { rsf_overlay_d3d12_stop() };
    }
    if debug {
        let clean = validation_clean(gpu) && passed;
        println!("D3D12 overlay/presentation validation={}", u32::from(clean));
        if !clean {
            return Err(1);
        }
    }
    Ok(i32::from(!passed))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_fixture_main(argc: i32, argv: *const *const c_char) -> i32 {
    unsafe { run(args(argc, argv)) }.unwrap_or_else(|status| status)
}
