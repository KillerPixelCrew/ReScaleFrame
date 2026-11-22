// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d11_d3d12 as graphics;
use crate::overlay_host_abi::{RSF_OVERLAY_INPUT_ABI_VERSION, rsf_overlay_input_options};
use crate::overlay_input::{
    rsf_overlay_input_collect, rsf_overlay_input_install, rsf_overlay_input_set_visible,
    rsf_overlay_input_uninstall, rsf_overlay_input_visible,
};
use crate::overlay_window::{NOT_INSTALLED, OK};
use rsf_shader_compiler::{Compiler, Request, blob_bytes, compile};
use std::mem::ManuallyDrop;
use std::ptr::copy_nonoverlapping;
use windows::{
    Win32::{
        Foundation::{HANDLE, RECT},
        Graphics::{
            Direct3D::{D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, Fxc::D3DCOMPILE_ENABLE_STRICTNESS},
            Direct3D12::*,
            Dxgi::{Common::*, IDXGISwapChain, IDXGISwapChain3},
        },
    },
    core::{BOOL, Interface, PCSTR},
};

struct Gpu {
    pixel: Vec<u8>,
    vertex: Vec<u8>,
    root: ID3D12RootSignature,
    queue: ID3D12CommandQueue,
    device: ID3D12Device,
    stride: u32,
}

const _: () = {
    assert!(size_of::<rsf_overlay_vertex>() == 20);
    assert!(std::mem::offset_of!(rsf_overlay_vertex, u) == 8);
    assert!(std::mem::offset_of!(rsf_overlay_vertex, color) == 16);
};

fn root_signature(device: &ID3D12Device) -> Option<ID3D12RootSignature> {
    let range = D3D12_DESCRIPTOR_RANGE {
        RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
        NumDescriptors: 1,
        ..Default::default()
    };
    let parameters = [
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                Constants: D3D12_ROOT_CONSTANTS {
                    Num32BitValues: 2,
                    ..Default::default()
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_VERTEX,
        },
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: 1,
                    pDescriptorRanges: &range,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        },
    ];
    let sampler = D3D12_STATIC_SAMPLER_DESC {
        Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        MaxLOD: D3D12_FLOAT32_MAX,
        ComparisonFunc: D3D12_COMPARISON_FUNC_ALWAYS,
        MaxAnisotropy: 1,
        ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        ..Default::default()
    };
    let desc = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: parameters.len() as u32,
        pParameters: parameters.as_ptr(),
        NumStaticSamplers: 1,
        pStaticSamplers: &sampler,
        Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
    };
    let mut blob = None;
    unsafe { D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None) }
        .ok()?;
    unsafe { device.CreateRootSignature(0, blob_bytes(blob.as_ref()?)) }.ok()
}

unsafe fn initialize(device: *mut c_void, queue: *mut c_void) -> Option<(Gpu, ID3D12Fence)> {
    let device = unsafe { graphics::borrow::<ID3D12Device>(&device) }.clone();
    let queue = unsafe { graphics::borrow::<ID3D12CommandQueue>(&queue) }.clone();
    let stride =
        unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) };
    let fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.ok()?;
    let compiler = Compiler::system("d3dcompiler_47.dll")?;
    let entry = compiler.entry()?;
    let shader = |name, target| {
        let output = unsafe {
            compile(
                entry,
                Request {
                    source: include_bytes!("overlay.hlsl"),
                    name: c"overlay",
                    entry: name,
                    target,
                    flags: D3DCOMPILE_ENABLE_STRICTNESS,
                },
                &[],
                null_mut(),
            )
        };
        if output.status < 0 { None } else { output.code }
    };
    let vertex = shader(c"vs", c"vs_5_0")?;
    let pixel = shader(c"ps", c"ps_5_0")?;
    let root = root_signature(&device)?;
    Some((
        Gpu {
            pixel,
            vertex,
            root,
            queue,
            device,
            stride,
        },
        fence,
    ))
}

unsafe extern "C" fn create_renderer(
    device: *mut c_void,
    queue: *mut c_void,
    fence: *mut *mut c_void,
) -> *mut c_void {
    let Some((gpu, completion)) = (unsafe { initialize(device, queue) }) else {
        return null_mut();
    };
    let gpu = crate::allocate_handle(gpu);
    if !gpu.is_null() {
        unsafe { fence.write(completion.into_raw()) };
    }
    gpu.cast()
}

unsafe extern "C" fn destroy(gpu: *mut c_void) {
    drop(unsafe { Box::from_raw(gpu.cast::<Gpu>()) });
}

unsafe extern "C" fn create_frame(gpu: *mut c_void, out: *mut FrameTable) -> u32 {
    let gpu = unsafe { &*gpu.cast::<Gpu>() };
    let out = unsafe { &mut *out };
    let kind = D3D12_COMMAND_LIST_TYPE_DIRECT;
    let Ok(allocator): Result<ID3D12CommandAllocator, _> =
        (unsafe { gpu.device.CreateCommandAllocator(kind) })
    else {
        return 0;
    };
    let list: windows::core::Result<ID3D12GraphicsCommandList> = unsafe {
        gpu.device
            .CreateCommandList(0, kind, &allocator, None::<&ID3D12PipelineState>)
    };
    out.allocator = allocator.into_raw();
    let Ok(list) = list else { return 0 };
    // The slot owner adopts partial outputs even when Close or a heap fails.
    let closed = unsafe { list.Close() }.is_ok();
    out.commands = list.into_raw();
    if !closed {
        return 0;
    }
    let mut desc = D3D12_DESCRIPTOR_HEAP_DESC {
        Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
        NumDescriptors: RSF_OVERLAY12_TEXTURE_LIMIT,
        Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
        NodeMask: 0,
    };
    let Ok(srv): Result<ID3D12DescriptorHeap, _> =
        (unsafe { gpu.device.CreateDescriptorHeap(&desc) })
    else {
        return 0;
    };
    out.srv = srv.into_raw();
    desc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    desc.NumDescriptors = 1;
    desc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_NONE;
    let Ok(rtv): Result<ID3D12DescriptorHeap, _> =
        (unsafe { gpu.device.CreateDescriptorHeap(&desc) })
    else {
        return 0;
    };
    out.rtv = rtv.into_raw();
    1
}

unsafe extern "C" fn create_pipeline(gpu: *mut c_void, format: u32) -> *mut c_void {
    let gpu = unsafe { &*gpu.cast::<Gpu>() };
    let layout = [
        (c"POSITION", DXGI_FORMAT_R32G32_FLOAT, 0),
        (c"TEXCOORD", DXGI_FORMAT_R32G32_FLOAT, 8),
        (c"COLOR", DXGI_FORMAT_R8G8B8A8_UNORM, 16),
    ]
    .map(|(name, format, offset)| D3D12_INPUT_ELEMENT_DESC {
        SemanticName: PCSTR(name.as_ptr().cast()),
        Format: format,
        AlignedByteOffset: offset,
        InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
        ..Default::default()
    });
    let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
        pRootSignature: ManuallyDrop::new(Some(unsafe {
            ID3D12RootSignature::from_raw(gpu.root.as_raw())
        })),
        VS: D3D12_SHADER_BYTECODE {
            pShaderBytecode: gpu.vertex.as_ptr().cast(),
            BytecodeLength: gpu.vertex.len(),
        },
        PS: D3D12_SHADER_BYTECODE {
            pShaderBytecode: gpu.pixel.as_ptr().cast(),
            BytecodeLength: gpu.pixel.len(),
        },
        InputLayout: D3D12_INPUT_LAYOUT_DESC {
            pInputElementDescs: layout.as_ptr(),
            NumElements: layout.len() as u32,
        },
        PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
        NumRenderTargets: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        SampleMask: u32::MAX,
        ..Default::default()
    };
    desc.RTVFormats[0] = DXGI_FORMAT(format as i32);
    desc.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
    desc.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
    desc.RasterizerState.DepthClipEnable = BOOL(1);
    desc.BlendState.RenderTarget[0] = D3D12_RENDER_TARGET_BLEND_DESC {
        BlendEnable: BOOL(1),
        SrcBlend: D3D12_BLEND_ONE,
        DestBlend: D3D12_BLEND_INV_SRC_ALPHA,
        BlendOp: D3D12_BLEND_OP_ADD,
        SrcBlendAlpha: D3D12_BLEND_ONE,
        DestBlendAlpha: D3D12_BLEND_INV_SRC_ALPHA,
        BlendOpAlpha: D3D12_BLEND_OP_ADD,
        RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
        ..Default::default()
    };
    desc.DepthStencilState.DepthFunc = D3D12_COMPARISON_FUNC_ALWAYS;
    desc.DepthStencilState.FrontFace = D3D12_DEPTH_STENCILOP_DESC {
        StencilFailOp: D3D12_STENCIL_OP_KEEP,
        StencilDepthFailOp: D3D12_STENCIL_OP_KEEP,
        StencilPassOp: D3D12_STENCIL_OP_KEEP,
        StencilFunc: D3D12_COMPARISON_FUNC_ALWAYS,
    };
    desc.DepthStencilState.BackFace = desc.DepthStencilState.FrontFace;
    unsafe {
        gpu.device
            .CreateGraphicsPipelineState::<ID3D12PipelineState>(&desc)
    }
    .map_or(null_mut(), Interface::into_raw)
}

fn upload_buffer(gpu: &Gpu, size: u64) -> Option<(ID3D12Resource, u64)> {
    if size == 0 {
        return None;
    }
    let capacity = size.checked_add(65535)? & !65535;
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_UPLOAD,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: capacity,
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
    let mut buffer = None;
    unsafe {
        gpu.device.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
            &mut buffer,
        )
    }
    .ok()?;
    Some((buffer?, capacity))
}

unsafe extern "C" fn create_upload(gpu: *mut c_void, size: u64, capacity: *mut u64) -> *mut c_void {
    let Some((buffer, bytes)) = upload_buffer(unsafe { &*gpu.cast::<Gpu>() }, size) else {
        return null_mut();
    };
    if !capacity.is_null() {
        unsafe { capacity.write(bytes) };
    }
    buffer.into_raw()
}

unsafe extern "C" fn create_texture(gpu: *mut c_void, width: u32, height: u32) -> *mut c_void {
    let gpu = unsafe { &*gpu.cast::<Gpu>() };
    unsafe {
        graphics::create_texture12(
            gpu.device.as_raw(),
            width,
            height,
            DXGI_FORMAT_R8G8B8A8_UNORM.0 as u32,
            D3D12_RESOURCE_FLAG_NONE,
            D3D12_RESOURCE_STATE_COPY_DEST,
        )
    }
}

fn with_mapping(resource: &ID3D12Resource, write: impl FnOnce(*mut u8)) -> Option<()> {
    let empty = D3D12_RANGE { Begin: 0, End: 0 };
    let mut mapped = null_mut();
    unsafe { resource.Map(0, Some(&empty), Some(&mut mapped)) }.ok()?;
    write(mapped.cast());
    unsafe { resource.Unmap(0, None) };
    Some(())
}

unsafe fn texture_upload(
    gpu: &Gpu,
    slot: &FrameTable,
    dst: *mut c_void,
    patch: &rsf_overlay_texture_update,
) -> Option<ID3D12Resource> {
    let destination = unsafe { graphics::borrow::<ID3D12Resource>(&dst) };
    let commands = unsafe { graphics::borrow::<ID3D12GraphicsCommandList>(&slot.commands) };
    let mut desc = unsafe { destination.GetDesc() };
    desc.Width = u64::from(patch.width);
    desc.Height = patch.height;
    let mut footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
    let mut bytes = 0;
    unsafe {
        gpu.device.GetCopyableFootprints(
            &desc,
            0,
            1,
            0,
            Some(&mut footprint),
            None,
            None,
            Some(&mut bytes),
        )
    };
    let (upload, _) = upload_buffer(gpu, bytes)?;
    let row_bytes = patch.width as usize * 4;
    with_mapping(&upload, |mapped| {
        for row in 0..patch.height as usize {
            unsafe {
                copy_nonoverlapping(
                    patch.pixels.add(row * row_bytes),
                    mapped.add(row * footprint.Footprint.RowPitch as usize),
                    row_bytes,
                )
            };
        }
    })?;
    let source = D3D12_TEXTURE_COPY_LOCATION {
        pResource: ManuallyDrop::new(Some(unsafe { ID3D12Resource::from_raw(upload.as_raw()) })),
        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            PlacedFootprint: footprint,
        },
    };
    let target = D3D12_TEXTURE_COPY_LOCATION {
        pResource: ManuallyDrop::new(Some(unsafe { ID3D12Resource::from_raw(dst) })),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: 0,
        },
    };
    let copy = D3D12_RESOURCE_STATE_COPY_DEST.0 as u32;
    let sample = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0 as u32;
    unsafe {
        if patch.is_whole_texture == 0 {
            graphics::transition(slot.commands, dst, sample, copy);
        }
        commands.CopyTextureRegion(&target, patch.x, patch.y, 0, &source, None);
        graphics::transition(slot.commands, dst, copy, sample);
    }
    Some(upload)
}

unsafe extern "C" fn upload_texture(
    gpu: *mut c_void,
    slot: *const FrameTable,
    dst: *mut c_void,
    patch: *const rsf_overlay_texture_update,
) -> *mut c_void {
    unsafe { texture_upload(&*gpu.cast::<Gpu>(), &*slot, dst, &*patch) }
        .map_or(null_mut(), Interface::into_raw)
}

unsafe fn draw(
    gpu: &Gpu,
    slot: &FrameTable,
    binding: &Draw,
    textures: &[*mut c_void],
    calls: &[DrawCall],
) -> Option<()> {
    if binding.vertex_bytes == 0 || binding.index_bytes == 0 {
        return Some(());
    }
    let list = unsafe { graphics::borrow::<ID3D12GraphicsCommandList>(&slot.commands) };
    let target = unsafe { graphics::borrow::<ID3D12Resource>(&binding.target) };
    let vertex = unsafe { graphics::borrow::<ID3D12Resource>(&binding.vertices) };
    let index = unsafe { graphics::borrow::<ID3D12Resource>(&binding.indices) };
    let srv = unsafe { graphics::borrow::<ID3D12DescriptorHeap>(&slot.srv) };
    let rtv = unsafe { graphics::borrow::<ID3D12DescriptorHeap>(&slot.rtv) };
    let pipeline = unsafe { graphics::borrow::<ID3D12PipelineState>(&binding.pipeline) };
    with_mapping(vertex, |mapped| unsafe {
        copy_nonoverlapping(
            binding.vertex_data.cast(),
            mapped,
            binding.vertex_bytes as usize,
        )
    })?;
    with_mapping(index, |mapped| unsafe {
        copy_nonoverlapping(
            binding.index_data.cast(),
            mapped,
            binding.index_bytes as usize,
        )
    })?;
    let mut cpu = unsafe { srv.GetCPUDescriptorHandleForHeapStart() };
    let view = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D12_TEX2D_SRV {
                MipLevels: 1,
                ..Default::default()
            },
        },
    };
    for texture in textures {
        let texture = unsafe { graphics::borrow::<ID3D12Resource>(texture) };
        unsafe {
            gpu.device
                .CreateShaderResourceView(texture, Some(&view), cpu)
        };
        cpu.ptr += gpu.stride as usize;
    }
    let present = D3D12_RESOURCE_STATE_PRESENT.0 as u32;
    let render = D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32;
    let rtv = unsafe { rtv.GetCPUDescriptorHandleForHeapStart() };
    let gpu_start = unsafe { srv.GetGPUDescriptorHandleForHeapStart() };
    let heaps = ManuallyDrop::new([Some(unsafe { ID3D12DescriptorHeap::from_raw(slot.srv) })]);
    let scale = [2.0 / binding.width as f32, 2.0 / binding.height as f32];
    let viewport = D3D12_VIEWPORT {
        Width: binding.width as f32,
        Height: binding.height as f32,
        MaxDepth: 1.0,
        ..Default::default()
    };
    unsafe {
        graphics::transition(slot.commands, binding.target, present, render);
        gpu.device.CreateRenderTargetView(target, None, rtv);
        list.OMSetRenderTargets(1, Some(&rtv), false, None);
        list.SetPipelineState(pipeline);
        list.SetGraphicsRootSignature(&gpu.root);
        list.SetDescriptorHeaps(&heaps[..]);
        list.SetGraphicsRoot32BitConstants(0, 2, scale.as_ptr().cast(), 0);
        list.RSSetViewports(&[viewport]);
        list.IASetVertexBuffers(
            0,
            Some(&[D3D12_VERTEX_BUFFER_VIEW {
                BufferLocation: vertex.GetGPUVirtualAddress(),
                SizeInBytes: binding.vertex_bytes,
                StrideInBytes: size_of::<rsf_overlay_vertex>() as u32,
            }]),
        );
        list.IASetIndexBuffer(Some(&D3D12_INDEX_BUFFER_VIEW {
            BufferLocation: index.GetGPUVirtualAddress(),
            SizeInBytes: binding.index_bytes,
            Format: DXGI_FORMAT_R32_UINT,
        }));
        list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        for call in calls {
            let [left, top, right, bottom] = call.clip;
            list.RSSetScissorRects(&[RECT {
                left,
                top,
                right,
                bottom,
            }]);
            list.SetGraphicsRootDescriptorTable(
                1,
                D3D12_GPU_DESCRIPTOR_HANDLE {
                    ptr: gpu_start.ptr + u64::from(call.descriptor) * u64::from(gpu.stride),
                },
            );
            list.DrawIndexedInstanced(call.index_count, 1, call.first_index, call.vertex_offset, 0);
        }
        graphics::transition(slot.commands, binding.target, render, present);
    }
    Some(())
}

unsafe extern "C" fn record(
    gpu: *mut c_void,
    slot: *const FrameTable,
    binding: *const Draw,
    textures: *const *mut c_void,
    texture_count: u32,
    calls: *const DrawCall,
    call_count: u32,
) -> u32 {
    let Some(textures) = (unsafe { borrowed_draw_slice(textures, texture_count) }) else {
        return 0;
    };
    let Some(calls) = (unsafe { borrowed_draw_slice(calls, call_count) }) else {
        return 0;
    };
    let slot = unsafe { &*slot };
    u32::from(unsafe {
        draw(&*gpu.cast::<Gpu>(), slot, &*binding, textures, calls).is_some()
            && graphics::close_commands(slot.commands) != 0
    })
}

unsafe extern "C" fn window(chain: *mut c_void) -> *mut c_void {
    unsafe { graphics::borrow::<IDXGISwapChain>(&chain).GetDesc() }
        .map_or(null_mut(), |desc| desc.OutputWindow.0)
}

unsafe extern "C" fn begin(
    slot: *const FrameTable,
    chain: *mut c_void,
    out: *mut Target,
) -> *mut c_void {
    let slot = unsafe { &*slot };
    if unsafe { graphics::reset_commands(slot.allocator, slot.commands) } == 0 {
        return null_mut();
    }
    let Ok(chain) =
        (unsafe { graphics::borrow::<IDXGISwapChain>(&chain) }).cast::<IDXGISwapChain3>()
    else {
        return null_mut();
    };
    let resource: windows::core::Result<ID3D12Resource> =
        unsafe { chain.GetBuffer(chain.GetCurrentBackBufferIndex()) };
    let Ok(resource) = resource else {
        return null_mut();
    };
    let desc = unsafe { resource.GetDesc() };
    if desc.Width == 0 || desc.Width > u64::from(u32::MAX) || desc.Height == 0 {
        return null_mut();
    }
    unsafe {
        out.write(Target {
            width: desc.Width as u32,
            height: desc.Height,
            format: desc.Format.0 as u32,
        })
    };
    resource.into_raw()
}

unsafe extern "C" fn install(
    window: *mut c_void,
    log: rsf_overlay12_log,
    user: *mut c_void,
) -> u32 {
    let options = rsf_overlay_input_options {
        struct_size: size_of::<rsf_overlay_input_options>() as u32,
        abi_version: RSF_OVERLAY_INPUT_ABI_VERSION,
        toggle_virtual_key: 0,
        log,
        log_user: user,
    };
    u32::from(unsafe { rsf_overlay_input_install(window, &options) } == OK)
}

unsafe extern "C" fn stop() -> u32 {
    u32::from(matches!(rsf_overlay_input_uninstall(), OK | NOT_INSTALLED))
}

unsafe extern "C" fn collect(
    input: *mut rsf_overlay_input,
    width: u32,
    height: u32,
    elapsed: f32,
) -> u32 {
    u32::from(unsafe { rsf_overlay_input_collect(input, width, height, elapsed) } == OK)
}

unsafe extern "C" fn completed(fence: *mut c_void) -> u64 {
    unsafe { graphics::borrow::<ID3D12Fence>(&fence).GetCompletedValue() }
}

unsafe extern "C" fn arm(fence: *mut c_void, event: *mut c_void, value: u64) -> u32 {
    u32::from(
        unsafe {
            graphics::borrow::<ID3D12Fence>(&fence).SetEventOnCompletion(value, HANDLE(event))
        }
        .is_ok(),
    )
}

unsafe extern "C" fn execute(gpu: *mut c_void, slot: *const FrameTable) {
    unsafe { graphics::execute((*gpu.cast::<Gpu>()).queue.as_raw(), (*slot).commands) };
}

unsafe extern "C" fn signal(gpu: *mut c_void, fence: *mut c_void, value: u64) -> u32 {
    unsafe { graphics::queue_signal((*gpu.cast::<Gpu>()).queue.as_raw(), fence, value) as u32 }
}

static PLATFORM: Platform = Platform {
    window: Some(window),
    initialize: Some(create_renderer),
    create_frame: Some(create_frame),
    install_input: Some(install),
    stop_input: Some(stop),
    destroy: Some(destroy),
    visible: Some(rsf_overlay_input_visible),
    set_visible: Some(rsf_overlay_input_set_visible),
    completed: Some(completed),
    arm: Some(arm),
    begin: Some(begin),
    create_pipeline: Some(create_pipeline),
    create_upload: Some(create_upload),
    collect_input: Some(collect),
    create_texture: Some(create_texture),
    upload_texture: Some(upload_texture),
    record: Some(record),
    execute: Some(execute),
    signal: Some(signal),
};

#[unsafe(no_mangle)]
extern "C" fn rsf_overlay_native12_visible() -> u32 {
    rsf_core_overlay12_visible(&PLATFORM)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_overlay_native12_toggle() {
    rsf_core_overlay12_toggle(&PLATFORM);
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_overlay_native12_present(
    device: *mut c_void,
    queue: *mut c_void,
    chain: *mut c_void,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    log: rsf_overlay12_log,
    user: *mut c_void,
) -> i32 {
    unsafe { rsf_core_overlay12_present(&PLATFORM, device, queue, chain, stats, intent, log, user) }
}
