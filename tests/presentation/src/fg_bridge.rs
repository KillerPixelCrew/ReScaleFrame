// SPDX-License-Identifier: MIT
//! Typed command recording for the linked FG fixture. The native harness owns
//! its vendor callbacks and the present and retirement order of those commands.
use crate::fg_device_runner_abi::*;
use crate::fixture_device_abi::{rsf_fg_fixture_inputs as FgInputs, rsf_fg_fixture_submission};
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, RSF_SCREEN_FLIGHT, rsf_camera_frame,
    rsf_frame_record, rsf_game_render_pass,
};
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use windows::Win32::Foundation::{HANDLE, RECT, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R32_FLOAT, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_WAS_STILL_DRAWING, DXGI_FRAME_STATISTICS, IDXGISwapChain, IDXGISwapChain3,
};
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::core::{BOOL, Interface};

type Raw = *mut c_void;

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_bridge_fixture_submit(
    frame: &rsf_fg_fixture_submission,
    accepted: &mut u32,
) -> i32 {
    let mut backbuffer = std::ptr::null_mut();
    if unsafe {
        rsf_fg_bridge_fixture_source_buffer(
            frame.chain,
            frame.allocator,
            frame.list,
            &mut backbuffer,
        )
    } != 0
    {
        return 1;
    }
    let backbuffer = unsafe { ID3D12Resource::from_raw(backbuffer) };
    if frame.produce_input != 0
        && let Some(prepare) = frame.prepare
    {
        unsafe {
            prepare(
                frame.prepare_user,
                frame.list,
                backbuffer.as_raw(),
                frame.session,
            )
        };
    }
    if frame.overlay != 0 {
        let list = unsafe { borrow::<ID3D12GraphicsCommandList>(&frame.list) };
        unsafe {
            barrier(
                list,
                backbuffer.as_raw(),
                D3D12_RESOURCE_STATE_PRESENT,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
            )
        };
    }
    if unsafe { rsf_fg_bridge_fixture_execute(frame.queue, frame.list) } != 0 {
        return 1;
    }
    if frame.submit != 0 {
        unsafe { rsf_fg12_submitted(1, frame.source) };
    }
    let (chain, source, view) = (frame.chain as usize, frame.source, frame.view);
    // The caller leases the chain through the worker's synchronous join.
    let callback = std::thread::Builder::new().spawn(move || {
        // The pass is a C record of scalar values and nullable resource pointers.
        let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
        pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
        pass.session_id = 1;
        pass.native_frame = source;
        pass.source_frame_id = source;
        pass.view_key = u64::from(view);
        pass.swapchain = chain as Raw;
        unsafe { rsf_fg12_window(&pass) };
    });
    if !callback.is_ok_and(|callback| callback.join().is_ok()) {
        return 1;
    }
    if frame.overlay != 0 {
        if frame.overlay_visible >= 0 {
            unsafe { rsf_overlay_input_set_visible(frame.overlay_visible as u32) };
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
                frame.gpu,
                frame.queue,
                frame.chain,
                &stats,
                &mut intent,
                frame.log,
                std::ptr::null_mut(),
            )
        };
        *accepted &= u32::from(drawn != 0);
        if unsafe { rsf_fg_bridge_fixture_reset(frame.finish_allocator, frame.finish_list) } != 0 {
            return 1;
        }
        let list = unsafe { borrow::<ID3D12GraphicsCommandList>(&frame.finish_list) };
        unsafe {
            barrier(
                list,
                backbuffer.as_raw(),
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                D3D12_RESOURCE_STATE_PRESENT,
            )
        };
        if unsafe { rsf_fg_bridge_fixture_execute(frame.queue, frame.finish_list) } != 0 {
            return 1;
        }
    }
    0
}

unsafe fn borrow<T: Interface>(value: &Raw) -> &T {
    unsafe { T::from_raw_borrowed(value) }.unwrap()
}

unsafe fn resource_ref(raw: Raw) -> ManuallyDrop<Option<ID3D12Resource>> {
    ManuallyDrop::new(Some(unsafe { ID3D12Resource::from_raw(raw) }))
}

unsafe fn barrier(
    list: &ID3D12GraphicsCommandList,
    resource: Raw,
    from: D3D12_RESOURCE_STATES,
    to: D3D12_RESOURCE_STATES,
) {
    let transition = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: unsafe { resource_ref(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: from,
                StateAfter: to,
            }),
        },
    };
    unsafe { list.ResourceBarrier(&[transition]) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_bridge_fixture_barrier(
    list: Raw,
    resource: Raw,
    from: u32,
    to: u32,
) {
    let list = unsafe { borrow::<ID3D12GraphicsCommandList>(&list) };
    unsafe {
        barrier(
            list,
            resource,
            D3D12_RESOURCE_STATES(from as i32),
            D3D12_RESOURCE_STATES(to as i32),
        )
    };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_bridge_fixture_inputs(
    list: Raw,
    depth: Raw,
    motion: Raw,
    cpu_heap: Raw,
    gpu_heap: Raw,
    stride: u32,
) {
    let commands = unsafe { borrow::<ID3D12GraphicsCommandList>(&list) };
    unsafe { (commands.vtable().SetDescriptorHeaps)(list, 1, &gpu_heap) };
    let mut cpu =
        unsafe { borrow::<ID3D12DescriptorHeap>(&cpu_heap).GetCPUDescriptorHandleForHeapStart() };
    let mut gpu =
        unsafe { borrow::<ID3D12DescriptorHeap>(&gpu_heap).GetGPUDescriptorHandleForHeapStart() };
    for texture in [depth, motion] {
        unsafe {
            barrier(
                commands,
                texture,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
            );
            commands.ClearUnorderedAccessViewFloat(
                gpu,
                cpu,
                borrow::<ID3D12Resource>(&texture),
                &[0.5, 0.0, 0.0, 0.0],
                &[],
            );
            barrier(
                commands,
                texture,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
            );
        }
        cpu.ptr += stride as usize;
        gpu.ptr += u64::from(stride);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_bridge_fixture_frame(
    backbuffer: Raw,
    depth: Raw,
    source: u64,
    input_qpc: u64,
    phase: u32,
    out: *mut rsf_frame_record,
) {
    let output = unsafe { borrow::<ID3D12Resource>(&backbuffer).GetDesc() };
    let render = unsafe { borrow::<ID3D12Resource>(&depth).GetDesc() };
    let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
    record.struct_size = size_of::<rsf_frame_record>() as u32;
    record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = 1;
    record.frame_id = source;
    record.resource_generation = 1;
    record.view_id = 1;
    record.input_qpc = input_qpc;
    record.phase = phase;
    record.screen = RSF_SCREEN_FLIGHT;
    record.output_width = output.Width as u32;
    record.output_height = output.Height;
    record.render_width = render.Width as u32;
    record.render_height = render.Height;
    record.frame_time_ms = 16.67;
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
    unsafe { out.write(record) };
}

unsafe fn resource_device(raw: Raw) -> Option<ID3D12Device> {
    let mut device = None;
    unsafe { borrow::<ID3D12Resource>(&raw).GetDevice(&mut device) }.ok()?;
    device
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_bridge_fixture_hudless(
    list: Raw,
    color: Raw,
    heap: Raw,
    frame: u64,
    mfg: u32,
    width: u32,
    height: u32,
) -> i32 {
    let Some(device) = (unsafe { resource_device(color) }) else {
        return 0;
    };
    let list = unsafe { borrow::<ID3D12GraphicsCommandList>(&list) };
    let target =
        unsafe { borrow::<ID3D12DescriptorHeap>(&heap).GetCPUDescriptorHandleForHeapStart() };
    unsafe {
        device.CreateRenderTargetView(borrow::<ID3D12Resource>(&color), None, target);
        barrier(
            list,
            color,
            D3D12_RESOURCE_STATE_PRESENT,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
        );
        list.ClearRenderTargetView(target, &[0.2, 0.4, 0.6, 1.0], None);
    }
    if mfg == 0 {
        return 1;
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
    1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_bridge_fixture_capture_flip(
    list: Raw,
    back: Raw,
    readback_slot: *mut Raw,
) -> i32 {
    let mut readback = unsafe { readback_slot.read() };
    if readback.is_null() {
        let Some(device) = (unsafe { resource_device(back) }) else {
            return 0;
        };
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_READBACK,
            ..Default::default()
        };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: 256,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let mut resource: Option<ID3D12Resource> = None;
        if unsafe {
            device.CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut resource,
            )
        }
        .is_err()
        {
            return 0;
        }
        let Some(resource) = resource else {
            return 0;
        };
        readback = resource.into_raw();
        unsafe { readback_slot.write(readback) };
    }
    let source = D3D12_TEXTURE_COPY_LOCATION {
        pResource: unsafe { resource_ref(back) },
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: 0,
        },
    };
    let target = D3D12_TEXTURE_COPY_LOCATION {
        pResource: unsafe { resource_ref(readback) },
        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                Offset: 0,
                Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    Width: 1,
                    Height: 1,
                    Depth: 1,
                    RowPitch: 256,
                },
            },
        },
    };
    let pixel = D3D12_BOX {
        left: 0,
        top: 0,
        front: 0,
        right: 1,
        bottom: 1,
        back: 1,
    };
    let list = unsafe { borrow::<ID3D12GraphicsCommandList>(&list) };
    unsafe {
        barrier(
            list,
            back,
            D3D12_RESOURCE_STATE_PRESENT,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        );
        list.CopyTextureRegion(&target, 0, 0, 0, &source, Some(&pixel));
        barrier(
            list,
            back,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            D3D12_RESOURCE_STATE_PRESENT,
        );
    }
    1
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_fg_bridge_fixture_source(frame: u64, switching: u32) -> u64 {
    if switching == 0 {
        frame
    } else {
        ((if frame < 21 { 1 } else { 2 }) << 32) | frame
    }
}

pub(super) struct FramePlan {
    pub(super) produce_input: u32,
    pub(super) submit: u32,
    pub(super) view: u32,
    pub(super) overlay_visible: i32,
    pub(super) expected_tag: u32,
    pub(super) expect_refusal: u32,
    pub(super) legacy_request: i32,
    pub(super) legacy_expected: i32,
    pub(super) hot_request: i32,
}

pub(super) fn frame_plan(
    frame: u64,
    hot: u32,
    mfg: u32,
    vendor: u32,
    current_backend: u32,
    has_streamline_directory: u32,
) -> FramePlan {
    let hot = hot != 0;
    let mfg = mfg != 0;
    FramePlan {
        produce_input: u32::from(!hot || frame != 7),
        submit: u32::from(hot || frame != 11),
        view: if !hot && frame == 10 { 2 } else { 1 },
        overlay_visible: match frame {
            2 | 16 => 1,
            8 | 25 => 0,
            _ => -1,
        },
        expected_tag: u32::from(if hot {
            current_backend != 0 && frame != 7
        } else {
            frame != 10 && frame != 11
        }),
        expect_refusal: u32::from(!hot && matches!(frame, 10 | 11)),
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
            35 if has_streamline_directory != 0 => 1,
            110 if mfg => 0,
            114 if mfg => 1,
            _ => -1,
        },
    }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_fg_bridge_fixture_mfg_count(frame: u64) -> u32 {
    match frame {
        45 => 2,
        60 | 100 => 3,
        75 => 5,
        90 => 1,
        _ => 0,
    }
}

unsafe fn allocate_inputs(
    device: &ID3D12Device,
    mixed: bool,
    hot: bool,
) -> windows::core::Result<FgInputs> {
    let mut desc = D3D12_DESCRIPTOR_HEAP_DESC {
        Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
        NumDescriptors: 2,
        ..Default::default()
    };
    let cpu: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap(&desc) }?;
    desc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    let gpu: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap(&desc) }?;
    let stride = unsafe { device.GetDescriptorHandleIncrementSize(desc.Type) };
    let color: Option<ID3D12DescriptorHeap> = if hot {
        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            NumDescriptors: 1,
            ..Default::default()
        };
        Some(unsafe { device.CreateDescriptorHeap(&desc) }?)
    } else {
        None
    };
    let mut cpu_slot = unsafe { cpu.GetCPUDescriptorHandleForHeapStart() };
    let mut visible_slot = unsafe { gpu.GetCPUDescriptorHandleForHeapStart() };
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
    let [depth, motion]: [Option<ID3D12Resource>; 2] = resources;
    let depth = depth.ok_or(windows::Win32::Foundation::E_POINTER)?;
    let motion = motion.ok_or(windows::Win32::Foundation::E_POINTER)?;
    Ok(FgInputs {
        cpu_heap: cpu.into_raw(),
        gpu_heap: gpu.into_raw(),
        color_heap: color.map_or(std::ptr::null_mut(), |heap| heap.into_raw()),
        depth: depth.into_raw(),
        motion: motion.into_raw(),
        stride,
    })
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_bridge_fixture_allocate(
    gpu: Raw,
    mixed: u32,
    hot: u32,
    output: *mut FgInputs,
) -> i32 {
    let Ok(inputs) =
        (unsafe { allocate_inputs(borrow::<ID3D12Device>(&gpu), mixed != 0, hot != 0) })
    else {
        return 1;
    };
    unsafe { output.write(inputs) };
    0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_bridge_fixture_commands(
    gpu: Raw,
    allocator: *mut Raw,
    list: *mut Raw,
) -> i32 {
    let device = unsafe { borrow::<ID3D12Device>(&gpu) };
    let ty = D3D12_COMMAND_LIST_TYPE_DIRECT;
    let Ok(slot) = (unsafe { device.CreateCommandAllocator::<ID3D12CommandAllocator>(ty) }) else {
        return 1;
    };
    let Ok(commands) = (unsafe {
        device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
            0,
            ty,
            &slot,
            None::<&ID3D12PipelineState>,
        )
    }) else {
        return 1;
    };
    if unsafe { commands.Close() }.is_err() {
        return 1;
    }
    unsafe {
        allocator.write(slot.into_raw());
        list.write(commands.into_raw());
    }
    0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_bridge_fixture_check_flip(readback: Raw, frame: u64) -> i32 {
    let Some(readback) = (unsafe { ID3D12Resource::from_raw_borrowed(&readback) }) else {
        return -1;
    };
    let mut pixel = std::ptr::null_mut();
    if unsafe { readback.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 4 }), Some(&mut pixel)) }
        .is_err()
    {
        return -1;
    }
    let rgba = (!pixel.is_null()).then(|| unsafe { pixel.cast::<[u8; 4]>().read() });
    unsafe { readback.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 })) };
    let Some([r, g, b, a]) = rgba else {
        return -1;
    };
    let expected = r == if frame % 2 != 0 { 255 } else { 0 };
    let matches = expected && (127..=128).contains(&g) && (63..=64).contains(&b) && a == 255;
    if !matches {
        eprintln!("FAIL: DX11 cached-view frame {frame} presented RGBA={r},{g},{b},{a}");
    }
    i32::from(matches)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_bridge_fixture_query(chain: Raw, previous: &mut u32) -> u32 {
    let chain = unsafe { borrow::<IDXGISwapChain>(&chain) };
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_bridge_fixture_reset(allocator: Raw, commands: Raw) -> i32 {
    let allocator = unsafe { borrow::<ID3D12CommandAllocator>(&allocator) };
    let commands = unsafe { borrow::<ID3D12GraphicsCommandList>(&commands) };
    let reset = unsafe { allocator.Reset() }
        .and_then(|()| unsafe { commands.Reset(allocator, None::<&ID3D12PipelineState>) });
    i32::from(reset.is_err())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_bridge_fixture_source_buffer(
    chain: Raw,
    allocator: Raw,
    commands: Raw,
    out: *mut Raw,
) -> i32 {
    let chain = unsafe { borrow::<IDXGISwapChain>(&chain) };
    let Ok(chain) = chain.cast::<IDXGISwapChain3>() else {
        return 1;
    };
    let index = unsafe { chain.GetCurrentBackBufferIndex() };
    let Ok(back) = (unsafe { chain.GetBuffer::<ID3D12Resource>(index) }) else {
        return 1;
    };
    if unsafe { rsf_fg_bridge_fixture_reset(allocator, commands) } != 0 {
        return 1;
    }
    unsafe { out.write(back.into_raw()) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_bridge_fixture_execute(queue: Raw, commands: Raw) -> i32 {
    let list = unsafe { borrow::<ID3D12GraphicsCommandList>(&commands) };
    if unsafe { list.Close() }.is_err() {
        return 1;
    }
    let bound = unsafe { borrow::<ID3D12CommandQueue>(&queue) };
    unsafe { (bound.vtable().ExecuteCommandLists)(queue, 1, &commands) };
    0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_bridge_fixture_drain(
    queue: Raw,
    fence: Raw,
    event: Raw,
    frame: u64,
) -> i32 {
    let Some(queue) = (unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }) else {
        return 1;
    };
    let fence = unsafe { borrow::<ID3D12Fence>(&fence) };
    let event = HANDLE(event);
    if unsafe { queue.Signal(fence, frame) }.is_err()
        || unsafe { fence.SetEventOnCompletion(frame, event) }.is_err()
        || unsafe { WaitForSingleObject(event, 10_000) } != WAIT_OBJECT_0
    {
        return 1;
    }
    0
}
