// SPDX-License-Identifier: MIT
//! Typed command recording for the linked FG fixture. The native harness owns
//! its vendor callbacks and the present and retirement order of those commands.
use crate::fg_fixture_callbacks::State;
use crate::sys::*;
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_SCREEN_FLIGHT, rsf_camera_frame, rsf_frame_record,
    rsf_game_render_pass,
};
use rsf_test_support::d3d12::{buffer, copy_location, transition, wait_for_signal};
use rsf_test_support::frame_record;
use std::ffi::c_void;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R32_FLOAT, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_WAS_STILL_DRAWING, DXGI_FRAME_STATISTICS, IDXGISwapChain, IDXGISwapChain3,
};
use windows::core::{BOOL, Interface};

type Raw = *mut c_void;

/// A command allocator and the list recorded through it.
pub(super) type Commands = (ID3D12CommandAllocator, ID3D12GraphicsCommandList);

/// The depth and motion textures and the descriptor heaps the fixture clears them through.
pub(super) struct Inputs {
    pub(super) depth: ID3D12Resource,
    pub(super) motion: ID3D12Resource,
    cpu_heap: ID3D12DescriptorHeap,
    gpu_heap: ID3D12DescriptorHeap,
    pub(super) color_heap: Option<ID3D12DescriptorHeap>,
    stride: u32,
}

/// One frame of the service loop: the source list to record and execute, the overlay's finishing
/// list, and the provider state its callbacks write to.
pub(super) struct Submission<'a> {
    pub(super) chain: &'a IDXGISwapChain,
    pub(super) gpu: &'a ID3D12Device,
    pub(super) queue: &'a ID3D12CommandQueue,
    pub(super) source_commands: &'a Commands,
    pub(super) finish_commands: Option<&'a Commands>,
    pub(super) session: Raw,
    pub(super) source: u64,
    pub(super) produce_input: bool,
    pub(super) submit: bool,
    pub(super) view: u32,
    pub(super) overlay_visible: Option<bool>,
    pub(super) state: &'a State,
    pub(super) log: rsf_backend_log_fn,
}

/// Records, executes and presents one service frame. `None` is a graphics error. Otherwise the
/// result is whether the overlay drew; a failed overlay draw does not end the run.
pub(super) fn submit(frame: &Submission) -> Option<bool> {
    let (allocator, list) = frame.source_commands;
    let backbuffer = source_buffer(frame.chain, allocator, list)?;
    if frame.produce_input {
        crate::fg_fixture_callbacks::prepare_input(frame.state, list, &backbuffer, frame.session);
    }
    let overlay = frame.finish_commands.is_some();
    if overlay {
        transition(
            list,
            &backbuffer,
            D3D12_RESOURCE_STATE_PRESENT,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
        );
    }
    execute(frame.queue, list)?;
    if frame.submit {
        unsafe { rsf_fg12_submitted(1, frame.source) };
    }
    let (chain, source, view) = (frame.chain.as_raw() as usize, frame.source, frame.view);
    // The caller leases the chain through the worker's synchronous join.
    let callback = std::thread::Builder::new().spawn(move || {
        // The pass is a C record of scalar values and nullable resource pointers.
        let pass = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            session_id: 1,
            native_frame: source,
            source_frame_id: source,
            view_key: u64::from(view),
            swapchain: chain as Raw,
            ..Default::default()
        };
        unsafe { rsf_fg12_window(&pass) };
    });
    callback.ok()?.join().ok()?;
    let Some((finish_allocator, finish_list)) = frame.finish_commands else {
        return Some(true);
    };
    if let Some(visible) = frame.overlay_visible {
        unsafe { rsf_overlay_input_set_visible(u32::from(visible)) };
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
    let drawn = unsafe {
        rsf_overlay_d3d12_frame(
            frame.gpu.as_raw(),
            frame.queue.as_raw(),
            frame.chain.as_raw(),
            &stats,
            &mut intent,
            frame.log,
            std::ptr::null_mut(),
        )
    } != 0;
    reset(finish_allocator, finish_list).then_some(())?;
    transition(
        finish_list,
        &backbuffer,
        D3D12_RESOURCE_STATE_RENDER_TARGET,
        D3D12_RESOURCE_STATE_PRESENT,
    );
    execute(frame.queue, finish_list)?;
    Some(drawn)
}

/// Clears depth and motion through the shader-visible heap and leaves them readable by the provider.
pub(super) fn record_inputs(commands: &ID3D12GraphicsCommandList, inputs: &Inputs) {
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
pub(super) fn frame(
    backbuffer: &ID3D12Resource,
    depth: &ID3D12Resource,
    source: u64,
    input_qpc: u64,
    phase: u32,
) -> rsf_frame_record {
    let output = unsafe { backbuffer.GetDesc() };
    let render = unsafe { depth.GetDesc() };
    let mut record = rsf_frame_record {
        resource_generation: 1,
        input_qpc,
        phase,
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

fn resource_device(resource: &ID3D12Resource) -> Option<ID3D12Device> {
    let mut device = None;
    unsafe { resource.GetDevice(&mut device) }.ok()?;
    device
}

/// Clears `color` to the HUD-less scene (with moving tiles for the MFG fixture) through `heap`.
/// False when the resource has no device.
pub(super) fn hudless(
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
    // Dynamic tiles keep the MFG fixture from classifying identical frames as a menu.
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
pub(super) fn capture_flip(
    list: &ID3D12GraphicsCommandList,
    back: &ID3D12Resource,
    readback: &mut Option<ID3D12Resource>,
) -> bool {
    if readback.is_none() {
        let created = resource_device(back)
            .and_then(|device| buffer(&device, 256, D3D12_HEAP_TYPE_READBACK).ok());
        *readback = created;
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

/// The source id the loop reports for `frame`: the frame itself, or with runtime switching a
/// provider epoch in the high word.
pub(super) fn source_id(frame: u64, switching: bool) -> u64 {
    if switching {
        ((if frame < 21 { 1 } else { 2 }) << 32) | frame
    } else {
        frame
    }
}

pub(super) struct FramePlan {
    pub(super) produce_input: bool,
    pub(super) submit: bool,
    pub(super) view: u32,
    pub(super) overlay_visible: Option<bool>,
    pub(super) expected_tag: u32,
    pub(super) expect_refusal: bool,
    pub(super) legacy_request: i32,
    pub(super) legacy_expected: i32,
    pub(super) hot_request: i32,
}

pub(super) fn frame_plan(
    frame: u64,
    hot: bool,
    mfg: bool,
    vendor: u32,
    current_backend: u32,
    has_streamline_directory: bool,
) -> FramePlan {
    FramePlan {
        produce_input: !hot || frame != 7,
        submit: hot || frame != 11,
        view: if !hot && frame == 10 { 2 } else { 1 },
        overlay_visible: match frame {
            2 | 16 => Some(true),
            8 | 25 => Some(false),
            _ => None,
        },
        expected_tag: u32::from(if hot {
            current_backend != 0 && frame != 7
        } else {
            frame != 10 && frame != 11
        }),
        expect_refusal: !hot && matches!(frame, 10 | 11),
        legacy_request: match frame {
            8 => 3,
            15 => 5,
            25 => 0,
            32 => 1,
            _ => -1,
        },
        legacy_expected: match frame {
            9 => 3,
            16 => 5,
            26 => 0,
            33 => 1,
            _ => -1,
        },
        hot_request: match frame {
            2 if current_backend == 0 => 3,
            8 => 0,
            15 => 5,
            25 => 3,
            34 if vendor == 0x10de => 4,
            35 if has_streamline_directory => 1,
            110 if mfg => 0,
            114 if mfg => 1,
            _ => -1,
        },
    }
}

/// The generated-frame count the MFG fixture requests before `frame`, or zero to keep the last.
pub(super) fn mfg_count(frame: u64) -> u32 {
    match frame {
        45 => 2,
        60 | 100 => 3,
        75 => 5,
        90 => 1,
        _ => 0,
    }
}

pub(super) fn allocate_inputs(
    device: &ID3D12Device,
    mixed: bool,
    hot: bool,
) -> windows::core::Result<Inputs> {
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
    let [depth, motion] = resources;
    let (Some(depth), Some(motion)) = (depth, motion) else {
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
pub(super) fn commands(device: &ID3D12Device) -> windows::core::Result<Commands> {
    let allocator: ID3D12CommandAllocator =
        unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }?;
    let list: ID3D12GraphicsCommandList = unsafe {
        device.CreateCommandList(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            &allocator,
            None::<&ID3D12PipelineState>,
        )
    }?;
    unsafe { list.Close() }?;
    Ok((allocator, list))
}

/// Whether the first texel in `readback` is the colour `frame`'s clear should have produced.
/// `None` when it cannot be read.
pub(super) fn check_flip(readback: &ID3D12Resource, frame: u64) -> Option<bool> {
    let mut pixel = std::ptr::null_mut();
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

/// Queries the swap chain the way an engine's worker threads do and returns how many results
/// were inconsistent. `previous` carries the last present count.
pub(super) fn query(chain: &IDXGISwapChain, previous: &mut u32) -> u32 {
    let mut bad = 0;
    let mut fullscreen = BOOL(0);
    if unsafe { chain.GetFullscreenState(Some(&mut fullscreen), None) }
        .is_err_and(|error| error.code() != DXGI_ERROR_WAS_STILL_DRAWING)
    {
        bad += 1;
    }
    if let Ok(count) = unsafe { chain.GetLastPresentCount() } {
        bad += u32::from(count < *previous);
        *previous = count;
    }
    let _ = unsafe { chain.GetFrameStatistics(&mut DXGI_FRAME_STATISTICS::default()) };
    bad
}

fn reset(allocator: &ID3D12CommandAllocator, commands: &ID3D12GraphicsCommandList) -> bool {
    unsafe { allocator.Reset() }
        .and_then(|()| unsafe { commands.Reset(allocator, None::<&ID3D12PipelineState>) })
        .is_ok()
}

/// Resets `commands` and returns the chain's current back buffer.
fn source_buffer(
    chain: &IDXGISwapChain,
    allocator: &ID3D12CommandAllocator,
    commands: &ID3D12GraphicsCommandList,
) -> Option<ID3D12Resource> {
    let chain = chain.cast::<IDXGISwapChain3>().ok()?;
    let back =
        unsafe { chain.GetBuffer::<ID3D12Resource>(chain.GetCurrentBackBufferIndex()) }.ok()?;
    reset(allocator, commands).then_some(back)
}

fn execute(queue: &ID3D12CommandQueue, commands: &ID3D12GraphicsCommandList) -> Option<()> {
    unsafe { commands.Close() }.ok()?;
    let list = commands.cast::<ID3D12CommandList>().ok()?;
    unsafe { queue.ExecuteCommandLists(&[Some(list)]) };
    Some(())
}

/// Signals `fence` to `frame` on `queue` and waits for the GPU to reach it.
pub(super) fn drain(queue: &ID3D12CommandQueue, fence: &ID3D12Fence, frame: u64) -> bool {
    wait_for_signal(queue, fence, frame, 10_000)
}
