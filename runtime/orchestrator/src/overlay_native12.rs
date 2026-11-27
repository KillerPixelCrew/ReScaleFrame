// SPDX-License-Identifier: MIT
//! The native D3D12 panel (overlay_native12.cpp): the shared panel drawn with its own D3D12
//! pipeline on the presenting queue, through a three-slot ring of command lists.
#![cfg_attr(test, allow(dead_code))]
use crate::d3d11_d3d12::{create_texture12, transition_barrier};
use crate::d3d12_fence::FenceState;
use crate::overlay_geometry::{
    borrowed_draw_slice, draw_clip, patch_fits, valid_indices, valid_texture,
};
use crate::overlay_input::{
    rsf_overlay_input_collect, rsf_overlay_input_install, rsf_overlay_input_set_visible,
    rsf_overlay_input_toggle, rsf_overlay_input_uninstall, rsf_overlay_input_visible,
};
use crate::overlay_panel::{OpenPanel, PanelEntries, elapsed_seconds, open_panel};
use crate::overlay_window::{NOT_INSTALLED, OK};
use crate::owned_win32_handle;
use crate::sr_session::has_capacity;
use crate::sys::{
    RSF_OVERLAY_ABI_VERSION, RSF_OVERLAY_INPUT_ABI_VERSION, rsf_overlay,
    rsf_overlay_draw_data as DrawData, rsf_overlay_input, rsf_overlay_input_options,
    rsf_overlay_vertex,
};
use crate::sys::{rsf_overlay_intent, rsf_overlay_stats, rsf_overlay_texture_update};
use rsf_shader_compiler::{Compiler, Request, blob_bytes, compile};
use std::ffi::{CStr, c_char, c_void};
use std::mem::ManuallyDrop;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::ptr::{NonNull, copy_nonoverlapping, null_mut};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::Foundation::{HANDLE, RECT};
use windows::Win32::Graphics::Direct3D::{
    D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, Fxc::D3DCOMPILE_ENABLE_STRICTNESS,
};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::{Common::*, IDXGISwapChain, IDXGISwapChain3};
use windows::core::{BOOL, Interface, PCSTR};
use windows_sys::Win32::{
    Foundation::WAIT_OBJECT_0,
    System::{
        SystemInformation::GetTickCount64,
        Threading::{CreateEventW, WaitForSingleObject},
    },
};

pub(crate) type OverlayLog = Option<unsafe extern "C" fn(*mut c_void, *const c_char)>;

const SLOTS: usize = 3;
const TEXTURES: usize = 64;
const ATLAS_BATCH: usize = 32;
const STARTUP_MS: u64 = 8000;
const STARTUP_FADE_MS: u64 = 1000;
const FENCE_WAIT_MS: u32 = 5000;
const RETRY_DELAY_MS: u64 = 1000;

const _: () = {
    assert!(size_of::<rsf_overlay_vertex>() == 20);
    assert!(std::mem::offset_of!(rsf_overlay_vertex, u) == 8);
    assert!(std::mem::offset_of!(rsf_overlay_vertex, color) == 16);
};

struct Ui {
    api: PanelEntries,
    handle: NonNull<rsf_overlay>,
    module: Option<OpenPanel>,
}

// The panel lock serializes all calls into this instance.
unsafe impl Send for Ui {}

impl Ui {
    fn open() -> Option<Self> {
        let module = open_panel().ok()?;
        let api = module.entries;
        let handle = NonNull::new(unsafe { (api.create)(RSF_OVERLAY_ABI_VERSION) })?;
        Some(Self {
            api,
            handle,
            module: Some(module),
        })
    }

    fn publish(&mut self) {
        if let Some(module) = self.module.take() {
            module.publish();
        }
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        unsafe { (self.api.destroy)(self.handle.as_ptr()) };
    }
}

#[derive(Default)]
struct Ring {
    completions: [u64; SLOTS],
    cursor: usize,
    serial: u64,
}

impl Ring {
    fn select(&self, completed: u64) -> (usize, u64) {
        let mut oldest = 0;
        for index in (0..SLOTS).map(|i| (self.cursor + i) % SLOTS) {
            let completion = self.completions[index];
            if completion <= completed {
                return (index, 0);
            }
            if completion < self.completions[oldest] {
                oldest = index;
            }
        }
        (oldest, self.completions[oldest])
    }

    fn slot_ready(completed: u64, wait_for: u64) -> bool {
        FenceState::check(completed, wait_for) == FenceState::Complete
    }

    fn submitted(&mut self, slot: usize, value: u64) {
        self.completions[slot] = value;
        self.cursor = (slot + 1) % SLOTS;
        self.serial = value;
    }
}

struct Fence {
    reference: ID3D12Fence,
    event: Option<OwnedHandle>,
    armed: Option<u64>,
}

fn retire_fence_event(armed: &mut Option<u64>, signaled: bool, completed: u64) -> bool {
    let Some(value) = *armed else { return true };
    let state = FenceState::check(completed, value);
    if state == FenceState::DeviceRemoved || (signaled && state == FenceState::Complete) {
        *armed = None;
    }
    armed.is_none()
}

impl Fence {
    fn completed(&self) -> u64 {
        unsafe { self.reference.GetCompletedValue() }
    }

    fn wait_for_slot(&mut self, value: u64) -> bool {
        if self.armed.is_some() {
            return false;
        }
        if self.event.is_none() {
            let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
            self.event = unsafe { owned_win32_handle(event) };
        }
        let Some(event) = &self.event else {
            return false;
        };
        let event = HANDLE(event.as_raw_handle());
        if unsafe { self.reference.SetEventOnCompletion(value, event) }.is_err() {
            return false;
        }
        self.armed = Some(value);
        // Keep the event with its fence after a timeout. A later GPU signal may still reach it.
        self.finish_event(FENCE_WAIT_MS) && Ring::slot_ready(self.completed(), value)
    }

    fn finish_event(&mut self, timeout: u32) -> bool {
        if self.armed.is_none() {
            return true;
        }
        let Some(event) = &self.event else {
            return false;
        };
        let signaled =
            unsafe { WaitForSingleObject(event.as_raw_handle(), timeout) } == WAIT_OBJECT_0;
        let completed = self.completed();
        retire_fence_event(&mut self.armed, signaled, completed)
    }
}

/// The overlay device's pipeline inputs: shaders, root signature and the presenting queue.
struct Gpu {
    device: ID3D12Device,
    queue: ID3D12CommandQueue,
    root: ID3D12RootSignature,
    vertex: Vec<u8>,
    pixel: Vec<u8>,
    stride: u32,
}

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

impl Gpu {
    fn create(device: &ID3D12Device, queue: &ID3D12CommandQueue) -> Option<(Self, ID3D12Fence)> {
        let stride = unsafe {
            device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV)
        };
        let fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.ok()?;
        let compiler = Compiler::system("d3dcompiler_47.dll")?;
        let entry = compiler.entry()?;
        let shader = |name, target| {
            let output = unsafe {
                compile(
                    entry,
                    Request {
                        source: include_bytes!("overlay_native12/overlay.hlsl"),
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
        let root = root_signature(device)?;
        let gpu = Self {
            device: device.clone(),
            queue: queue.clone(),
            root,
            vertex,
            pixel,
            stride,
        };
        Some((gpu, fence))
    }

    fn pipeline(&self, format: DXGI_FORMAT) -> Option<ID3D12PipelineState> {
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
                ID3D12RootSignature::from_raw(self.root.as_raw())
            })),
            VS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: self.vertex.as_ptr().cast(),
                BytecodeLength: self.vertex.len(),
            },
            PS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: self.pixel.as_ptr().cast(),
                BytecodeLength: self.pixel.len(),
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
        desc.RTVFormats[0] = format;
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
        unsafe { self.device.CreateGraphicsPipelineState(&desc) }.ok()
    }

    fn upload_buffer(&self, size: u64) -> Option<(ID3D12Resource, u64)> {
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
            self.device.CreateCommittedResource(
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

    fn texture(&self, width: u32, height: u32) -> Option<ID3D12Resource> {
        let raw = unsafe {
            create_texture12(
                self.device.as_raw(),
                width,
                height,
                DXGI_FORMAT_R8G8B8A8_UNORM.0 as u32,
                D3D12_RESOURCE_FLAG_NONE,
                D3D12_RESOURCE_STATE_COPY_DEST,
            )
        };
        (!raw.is_null()).then(|| unsafe { ID3D12Resource::from_raw(raw) })
    }

    // Stages one atlas patch and records its copy into the slot's list.
    unsafe fn upload_texture(
        &self,
        list: &ID3D12GraphicsCommandList,
        destination: &ID3D12Resource,
        patch: &rsf_overlay_texture_update,
    ) -> Option<ID3D12Resource> {
        let mut desc = unsafe { destination.GetDesc() };
        desc.Width = u64::from(patch.width);
        desc.Height = patch.height;
        let mut footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
        let mut bytes = 0;
        unsafe {
            self.device.GetCopyableFootprints(
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
        let (upload, _) = self.upload_buffer(bytes)?;
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
            pResource: ManuallyDrop::new(Some(unsafe {
                ID3D12Resource::from_raw(upload.as_raw())
            })),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: footprint,
            },
        };
        let target = D3D12_TEXTURE_COPY_LOCATION {
            pResource: ManuallyDrop::new(Some(unsafe {
                ID3D12Resource::from_raw(destination.as_raw())
            })),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: 0,
            },
        };
        let (copy, sample) = (
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
        );
        unsafe {
            if patch.is_whole_texture == 0 {
                list.ResourceBarrier(&[transition_barrier(destination, sample, copy)]);
            }
            list.CopyTextureRegion(&target, patch.x, patch.y, 0, &source, None);
            list.ResourceBarrier(&[transition_barrier(destination, copy, sample)]);
        }
        Some(upload)
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

/// One Present slot: direct allocator, its closed list, a shader-visible SRV heap of `TEXTURES`
/// descriptors and a one-entry RTV heap.
struct Frame {
    allocator: ID3D12CommandAllocator,
    commands: ID3D12GraphicsCommandList,
    srv: ID3D12DescriptorHeap,
    rtv: ID3D12DescriptorHeap,
}

impl Frame {
    fn create(device: &ID3D12Device) -> Option<Self> {
        let kind = D3D12_COMMAND_LIST_TYPE_DIRECT;
        let allocator: ID3D12CommandAllocator =
            unsafe { device.CreateCommandAllocator(kind) }.ok()?;
        let commands: ID3D12GraphicsCommandList =
            unsafe { device.CreateCommandList(0, kind, &allocator, None::<&ID3D12PipelineState>) }
                .ok()?;
        unsafe { commands.Close() }.ok()?;
        let mut desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: TEXTURES as u32,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            NodeMask: 0,
        };
        let srv = unsafe { device.CreateDescriptorHeap(&desc) }.ok()?;
        desc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
        desc.NumDescriptors = 1;
        desc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_NONE;
        let rtv = unsafe { device.CreateDescriptorHeap(&desc) }.ok()?;
        Some(Self {
            allocator,
            commands,
            srv,
            rtv,
        })
    }

    // Reopens the list and returns the chain's current back buffer with its extent and format.
    fn begin(&self, chain: &IDXGISwapChain) -> Option<(ID3D12Resource, u32, u32, DXGI_FORMAT)> {
        unsafe {
            self.allocator.Reset().ok()?;
            self.commands.Reset(&self.allocator, None).ok()?;
        }
        let chain = chain.cast::<IDXGISwapChain3>().ok()?;
        let target: ID3D12Resource =
            unsafe { chain.GetBuffer(chain.GetCurrentBackBufferIndex()) }.ok()?;
        let desc = unsafe { target.GetDesc() };
        if desc.Width == 0 || desc.Width > u64::from(u32::MAX) || desc.Height == 0 {
            return None;
        }
        Some((target, desc.Width as u32, desc.Height, desc.Format))
    }
}

struct Upload {
    resource: ID3D12Resource,
    capacity: u64,
}

fn upload(buffer: &mut Option<Upload>, gpu: &Gpu, bytes: u32) -> Option<ID3D12Resource> {
    let bytes = u64::from(bytes);
    if let Some(buffer) = buffer.as_ref().filter(|buffer| buffer.capacity >= bytes) {
        return Some(buffer.resource.clone());
    }
    let (resource, capacity) = gpu.upload_buffer(bytes)?;
    *buffer = Some(Upload {
        resource: resource.clone(),
        capacity,
    });
    Some(resource)
}

/// The vertex and index uploads of one non-empty draw.
struct Geometry {
    vertices: ID3D12Resource,
    indices: ID3D12Resource,
    vertex_bytes: u32,
    index_bytes: u32,
}

#[derive(Default)]
struct DrawSlot {
    pipeline: Option<ID3D12PipelineState>,
    vertices: Option<Upload>,
    indices: Option<Upload>,
}

impl DrawSlot {
    // None refuses the frame; Some(None) is an empty draw.
    fn bind_uploads(&mut self, gpu: &Gpu, data: &DrawData) -> Option<Option<Geometry>> {
        if data.vertex_count == 0 || data.index_count == 0 {
            return Some(None);
        }
        if data.vertices.is_null() || data.indices.is_null() {
            return None;
        }
        let vertex_bytes = data
            .vertex_count
            .checked_mul(size_of::<rsf_overlay_vertex>() as u32)?;
        let index_bytes = data.index_count.checked_mul(size_of::<u32>() as u32)?;
        Some(Some(Geometry {
            vertices: upload(&mut self.vertices, gpu, vertex_bytes)?,
            indices: upload(&mut self.indices, gpu, index_bytes)?,
            vertex_bytes,
            index_bytes,
        }))
    }
}

#[derive(Default)]
struct Graphics {
    pipeline: Option<(DXGI_FORMAT, ID3D12PipelineState)>,
    slots: [DrawSlot; SLOTS],
}

impl Graphics {
    fn bind_pipeline(
        &mut self,
        gpu: &Gpu,
        slot: usize,
        format: DXGI_FORMAT,
    ) -> Option<ID3D12PipelineState> {
        if self
            .pipeline
            .as_ref()
            .is_none_or(|(active, _)| *active != format)
        {
            self.pipeline = Some((format, gpu.pipeline(format)?));
        }
        let (_, pipeline) = self.pipeline.as_ref()?;
        self.slots[slot].pipeline = Some(pipeline.clone());
        Some(pipeline.clone())
    }
}

struct Texture {
    id: u64,
    width: u32,
    height: u32,
    resource: ID3D12Resource,
}

// The target and geometry of one recorded panel draw.
struct Draw<'a> {
    target: &'a ID3D12Resource,
    pipeline: &'a ID3D12PipelineState,
    width: u32,
    height: u32,
    geometry: Option<Geometry>,
}

struct DrawCall {
    descriptor: u32,
    first_index: u32,
    index_count: u32,
    vertex_offset: i32,
    clip: [i32; 4],
}

#[derive(Default)]
struct Atlas {
    textures: Vec<Texture>,
    // What each slot's list reads, kept until the ring proves that slot complete.
    retained: [Vec<ID3D12Resource>; SLOTS],
}

impl Atlas {
    fn position(&self, id: u64) -> Option<usize> {
        self.textures.iter().position(|entry| entry.id == id)
    }

    fn update(
        &mut self,
        gpu: &Gpu,
        frame: &Frame,
        slot: usize,
        update: &rsf_overlay_texture_update,
    ) -> Option<()> {
        if !valid_texture(update) {
            return None;
        }
        let position = self.position(update.id);
        let replacement = if update.is_whole_texture != 0 {
            if update.x != 0
                || update.y != 0
                || (position.is_none() && self.textures.len() == TEXTURES)
            {
                return None;
            }
            Some(Texture {
                id: update.id,
                width: update.width,
                height: update.height,
                resource: gpu.texture(update.width, update.height)?,
            })
        } else {
            None
        };
        let texture = match replacement.as_ref() {
            Some(texture) => texture,
            None => &self.textures[position?],
        };
        if !patch_fits(update, texture.width, texture.height) {
            return None;
        }
        let upload = unsafe { gpu.upload_texture(&frame.commands, &texture.resource, update) }?;
        self.retained[slot].push(upload);
        self.retained[slot].push(texture.resource.clone());
        if let Some(replacement) = replacement {
            match position {
                Some(index) => self.textures[index] = replacement,
                None => self.textures.push(replacement),
            }
        }
        Some(())
    }

    unsafe fn draw_calls(&self, data: &DrawData, draw: &Draw<'_>) -> Option<Vec<DrawCall>> {
        if draw.geometry.is_none() {
            return Some(Vec::new());
        }
        let indices = unsafe { borrowed_draw_slice(data.indices, data.index_count) }?;
        let calls = unsafe { borrowed_draw_slice(data.calls, data.call_count) }?;
        let mut planned = Vec::new();
        planned.try_reserve_exact(calls.len()).ok()?;
        for call in calls {
            let Some(clip) = draw_clip(call, draw.width, draw.height).ok()? else {
                continue;
            };
            if !valid_indices(call, indices, data.vertex_count) {
                return None;
            }
            let descriptor = self.position(call.texture_id)? as u32;
            planned.push(DrawCall {
                descriptor,
                first_index: call.index_offset,
                index_count: call.index_count,
                vertex_offset: i32::try_from(call.vertex_offset).ok()?,
                clip,
            });
        }
        Some(planned)
    }

    // data borrows the live panel layout while this slot is recorded.
    unsafe fn record(
        &mut self,
        gpu: &Gpu,
        ui: &Ui,
        frame: &Frame,
        slot: usize,
        draw: &Draw<'_>,
        data: &DrawData,
    ) -> Option<()> {
        let mut batch = [rsf_overlay_texture_update::default(); ATLAS_BATCH];
        loop {
            let count = unsafe {
                (ui.api.texture_updates)(ui.handle.as_ptr(), batch.as_mut_ptr(), ATLAS_BATCH as u32)
            };
            if count == 0 {
                break;
            }
            for update in batch.get(..count as usize)? {
                self.update(gpu, frame, slot, update)?;
            }
        }

        let calls = unsafe { self.draw_calls(data, draw) }?;
        self.retained[slot].extend(self.textures.iter().map(|texture| texture.resource.clone()));
        if let Some(geometry) = &draw.geometry {
            unsafe { self.record_draw(gpu, frame, draw, geometry, data, &calls) }?;
        }
        unsafe { frame.commands.Close() }.ok()?;

        let mut ids = [0; ATLAS_BATCH];
        loop {
            let count = unsafe {
                (ui.api.textures_to_free)(ui.handle.as_ptr(), ids.as_mut_ptr(), ATLAS_BATCH as u32)
            };
            if count == 0 {
                return Some(());
            }
            for &id in ids.get(..count as usize)? {
                if let Some(index) = self.position(id) {
                    self.textures.swap_remove(index);
                }
            }
        }
    }

    unsafe fn record_draw(
        &self,
        gpu: &Gpu,
        frame: &Frame,
        draw: &Draw<'_>,
        geometry: &Geometry,
        data: &DrawData,
        calls: &[DrawCall],
    ) -> Option<()> {
        let list = &frame.commands;
        with_mapping(&geometry.vertices, |mapped| unsafe {
            copy_nonoverlapping(data.vertices.cast(), mapped, geometry.vertex_bytes as usize)
        })?;
        with_mapping(&geometry.indices, |mapped| unsafe {
            copy_nonoverlapping(data.indices.cast(), mapped, geometry.index_bytes as usize)
        })?;
        let mut cpu = unsafe { frame.srv.GetCPUDescriptorHandleForHeapStart() };
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
        for texture in &self.textures {
            unsafe {
                gpu.device
                    .CreateShaderResourceView(&texture.resource, Some(&view), cpu)
            };
            cpu.ptr += gpu.stride as usize;
        }
        let rtv = unsafe { frame.rtv.GetCPUDescriptorHandleForHeapStart() };
        let gpu_start = unsafe { frame.srv.GetGPUDescriptorHandleForHeapStart() };
        let scale = [2.0 / draw.width as f32, 2.0 / draw.height as f32];
        let viewport = D3D12_VIEWPORT {
            Width: draw.width as f32,
            Height: draw.height as f32,
            MaxDepth: 1.0,
            ..Default::default()
        };
        let (present, render) = (
            D3D12_RESOURCE_STATE_PRESENT,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
        );
        unsafe {
            list.ResourceBarrier(&[transition_barrier(draw.target, present, render)]);
            gpu.device.CreateRenderTargetView(draw.target, None, rtv);
            list.OMSetRenderTargets(1, Some(&rtv), false, None);
            list.SetPipelineState(draw.pipeline);
            list.SetGraphicsRootSignature(&gpu.root);
            list.SetDescriptorHeaps(&[Some(frame.srv.clone())]);
            list.SetGraphicsRoot32BitConstants(0, 2, scale.as_ptr().cast(), 0);
            list.RSSetViewports(&[viewport]);
            list.IASetVertexBuffers(
                0,
                Some(&[D3D12_VERTEX_BUFFER_VIEW {
                    BufferLocation: geometry.vertices.GetGPUVirtualAddress(),
                    SizeInBytes: geometry.vertex_bytes,
                    StrideInBytes: size_of::<rsf_overlay_vertex>() as u32,
                }]),
            );
            list.IASetIndexBuffer(Some(&D3D12_INDEX_BUFFER_VIEW {
                BufferLocation: geometry.indices.GetGPUVirtualAddress(),
                SizeInBytes: geometry.index_bytes,
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
                list.DrawIndexedInstanced(
                    call.index_count,
                    1,
                    call.first_index,
                    call.vertex_offset,
                    0,
                );
            }
            list.ResourceBarrier(&[transition_barrier(draw.target, render, present)]);
        }
        Some(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Owner {
    device: usize,
    queue: usize,
}

struct Retry {
    owner: Owner,
    after: u64,
}

impl Retry {
    fn allows(&self, owner: Owner, now: u64) -> bool {
        owner != self.owner || now >= self.after
    }
}

struct Started {
    // Fields release in declaration order: what the slots read, the UI, the slot lists and the
    // device objects, then the fence.
    atlas: Atlas,
    graphics: Graphics,
    ui: Ui,
    frames: Vec<Frame>,
    gpu: Gpu,
    fence: Fence,
    owner: Owner,
    failed: bool,
    unsignalled: Option<u64>,
    ring: Ring,
    started: u64,
    previous: u64,
    was_visible: bool,
    trace_left: u32,
    logger: OverlayLog,
    log_user: usize,
}

// The D3D12 references are free-threaded and the event handle may move; PANEL serializes all use.
unsafe impl Send for Started {}

struct Panel {
    started: Option<Started>,
    retry: Option<Retry>,
}

static READY: AtomicBool = AtomicBool::new(false);
static PANEL: Mutex<Panel> = Mutex::new(Panel {
    started: None,
    retry: None,
});

impl Started {
    fn initialize(
        device: &ID3D12Device,
        queue: &ID3D12CommandQueue,
        chain: &IDXGISwapChain,
        logger: OverlayLog,
        user: *mut c_void,
    ) -> Option<Self> {
        let window = unsafe { chain.GetDesc() }.ok()?.OutputWindow.0;
        if window.is_null() {
            return None;
        }
        let mut ui = Ui::open()?;
        let (gpu, fence) = Gpu::create(device, queue)?;
        let frames = (0..SLOTS)
            .map(|_| Frame::create(&gpu.device))
            .collect::<Option<Vec<_>>>()?;
        let options = rsf_overlay_input_options {
            struct_size: size_of::<rsf_overlay_input_options>() as u32,
            abi_version: RSF_OVERLAY_INPUT_ABI_VERSION,
            toggle_virtual_key: 0,
            log: logger,
            log_user: user,
        };
        if unsafe { rsf_overlay_input_install(window, &options) } != OK {
            return None;
        }
        ui.publish();
        Some(Self {
            atlas: Atlas::default(),
            graphics: Graphics::default(),
            ui,
            frames,
            gpu,
            fence: Fence {
                reference: fence,
                event: None,
                armed: None,
            },
            owner: Owner {
                device: device.as_raw() as usize,
                queue: queue.as_raw() as usize,
            },
            failed: false,
            unsignalled: None,
            ring: Ring::default(),
            started: unsafe { GetTickCount64() },
            previous: 0,
            was_visible: false,
            trace_left: 0,
            logger,
            log_user: user as usize,
        })
    }

    fn log(&self, message: &CStr) {
        unsafe {
            crate::graphics_d3d11::rsf_call_native_log(
                self.logger,
                self.log_user as *mut c_void,
                message.as_ptr(),
            );
        }
    }

    fn fail(&mut self) {
        self.failed = true;
        READY.store(false, Ordering::Release);
        rsf_overlay_input_set_visible(0);
        self.log(c"Native D3D12 overlay refused graphics work; see validation evidence.");
    }

    fn retired(&mut self) -> bool {
        let completed = self.fence.completed();
        if completed != u64::MAX {
            if let Some(value) = self.unsignalled {
                // Signal on the retained original queue behind the already executed list.
                if unsafe { self.gpu.queue.Signal(&self.fence.reference, value) }.is_ok() {
                    self.ring.serial = value;
                    self.unsignalled = None;
                }
                return false;
            }
            if completed < self.ring.serial {
                return false;
            }
        }
        self.fence.finish_event(0) && matches!(rsf_overlay_input_uninstall(), OK | NOT_INSTALLED)
    }

    unsafe fn present(
        &mut self,
        chain: &IDXGISwapChain,
        stats: *const rsf_overlay_stats,
    ) -> Option<rsf_overlay_intent> {
        let now = unsafe { GetTickCount64() };
        let active_for = now.saturating_sub(self.started);
        let visible = rsf_overlay_input_visible() != 0;
        if !visible && unsafe { (*stats).show_performance_hud } == 0 && active_for >= STARTUP_MS {
            return None;
        }
        // Every refusal after this point is a graphics failure.
        let decided = unsafe { self.record_frame(chain, stats, now, active_for, visible) };
        if decided.is_none() {
            self.fail();
        }
        decided
    }

    unsafe fn record_frame(
        &mut self,
        chain: &IDXGISwapChain,
        stats: *const rsf_overlay_stats,
        now: u64,
        active_for: u64,
        visible: bool,
    ) -> Option<rsf_overlay_intent> {
        let completed = self.fence.completed();
        if completed == u64::MAX {
            return None;
        }
        let (slot, wait_for) = self.ring.select(completed);
        // Only already-submitted overlay work can block slot reuse.
        if wait_for != 0 && !self.fence.wait_for_slot(wait_for) {
            return None;
        }
        // The fence proof above admits reuse of the slot's list and what it read.
        let frame = &self.frames[slot];
        self.atlas.retained[slot].clear();
        self.graphics.slots[slot].pipeline = None;
        let (target, width, height, format) = frame.begin(chain)?;
        let pipeline = self.graphics.bind_pipeline(&self.gpu, slot, format)?;
        if visible && !self.was_visible {
            self.trace_left = 4;
        }
        self.was_visible = visible;
        let trace = visible && self.trace_left != 0;
        if trace {
            self.trace_left -= 1;
            self.log(c"Overlay12: input/layout begin");
        }
        let elapsed = elapsed_seconds(if self.previous == 0 {
            0.0
        } else {
            now.saturating_sub(self.previous) as f64 / 1000.0
        });
        let mut decided = rsf_overlay_intent {
            struct_size: size_of::<rsf_overlay_intent>() as u32,
            ..Default::default()
        };
        let mut data = DrawData {
            struct_size: size_of::<DrawData>() as u32,
            ..Default::default()
        };
        let mut input = rsf_overlay_input {
            struct_size: size_of::<rsf_overlay_input>() as u32,
            ..Default::default()
        };
        if unsafe { rsf_overlay_input_collect(&mut input, width, height, elapsed) } != OK {
            return None;
        }
        input.startup_hint_alpha =
            STARTUP_MS.saturating_sub(active_for).min(STARTUP_FADE_MS) as f32 / 1000.0;
        let ui = &self.ui;
        if unsafe { (ui.api.frame)(ui.handle.as_ptr(), &input, stats, &mut data, &mut decided) }
            != 0
        {
            return None;
        }
        self.previous = now;
        if decided.close_requested != 0 {
            rsf_overlay_input_set_visible(0);
        }
        if trace {
            self.log(c"Overlay12: atlas uploads and draw recording");
        }
        let draw = Draw {
            target: &target,
            pipeline: &pipeline,
            width,
            height,
            geometry: self.graphics.slots[slot].bind_uploads(&self.gpu, &data)?,
        };
        unsafe {
            self.atlas
                .record(&self.gpu, &self.ui, frame, slot, &draw, &data)
        }?;
        let value = self
            .ring
            .serial
            .checked_add(1)
            .filter(|value| *value != u64::MAX)?;
        if trace {
            self.log(c"Overlay12: queue submission begin");
        }
        let list = Some(frame.commands.cast::<ID3D12CommandList>().ok()?);
        unsafe { self.gpu.queue.ExecuteCommandLists(&[list]) };
        if unsafe { self.gpu.queue.Signal(&self.fence.reference, value) }.is_err() {
            // Execute already happened. The failed owner stays alive until a signal proves retirement.
            self.unsignalled = Some(value);
            return None;
        }
        self.ring.submitted(slot, value);
        if trace {
            self.log(c"Overlay12: submission completed");
        }
        Some(decided)
    }
}

impl Panel {
    fn defer_retry(&mut self, owner: Owner, now: u64) {
        self.retry = Some(Retry {
            owner,
            after: now.saturating_add(RETRY_DELAY_MS),
        });
    }

    fn retire_failed(&mut self, now: u64) {
        if let Some(started) = &mut self.started
            && started.failed
            && started.retired()
        {
            let owner = started.owner;
            self.started = None;
            self.defer_retry(owner, now);
        }
    }
}

pub(crate) fn rsf_overlay_native12_visible() -> u32 {
    if READY.load(Ordering::Acquire) {
        rsf_overlay_input_visible()
    } else {
        0
    }
}

pub(crate) fn rsf_overlay_native12_toggle() {
    if let Ok(_panel) = PANEL.try_lock()
        && READY.load(Ordering::Acquire)
    {
        rsf_overlay_input_toggle();
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_overlay_native12_present(
    device: *mut c_void,
    queue: *mut c_void,
    chain: *mut c_void,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    logger: OverlayLog,
    user: *mut c_void,
) -> i32 {
    let (Some(device), Some(queue), Some(chain)) = (
        unsafe { ID3D12Device::from_raw_borrowed(&device) },
        unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) },
        unsafe { IDXGISwapChain::from_raw_borrowed(&chain) },
    ) else {
        return 0;
    };
    if !unsafe { has_capacity(stats) } || (!intent.is_null() && !unsafe { has_capacity(intent) }) {
        return 0;
    }
    let Ok(mut panel) = PANEL.try_lock() else {
        return 0;
    };
    let owner = Owner {
        device: device.as_raw() as usize,
        queue: queue.as_raw() as usize,
    };
    let now = unsafe { GetTickCount64() };
    panel.retire_failed(now);
    if panel.started.is_none() {
        if panel
            .retry
            .as_ref()
            .is_some_and(|retry| !retry.allows(owner, now))
        {
            return 0;
        }
        let Some(started) = Started::initialize(device, queue, chain, logger, user) else {
            panel.defer_retry(owner, now);
            unsafe {
                crate::graphics_d3d11::rsf_call_native_log(
                    logger,
                    user,
                    c"Native D3D12 overlay initialization failed.".as_ptr(),
                )
            };
            return 0;
        };
        READY.store(true, Ordering::Release);
        started.log(
            c"RSF native D3D12 overlay ready; Insert opens settings. No D3D11-on-12 resources.",
        );
        panel.started = Some(started);
        panel.retry = None;
    }
    let Some(started) = &mut panel.started else {
        return 0;
    };
    if started.failed || started.owner != owner {
        return 0;
    }
    if let Some(decided) = unsafe { started.present(chain, stats) } {
        if !intent.is_null() {
            unsafe { intent.write(decided) };
        }
        return 1;
    }
    if started.failed {
        panel.retire_failed(now);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_ring_waits_its_oldest_submitted_fence() {
        let mut ring = Ring::default();
        for index in 0..SLOTS {
            assert_eq!(ring.select(0), (index, 0));
            ring.submitted(index, index as u64 + 1);
        }
        assert_eq!(ring.select(0), (0, 1));
        assert!(!Ring::slot_ready(0, 1));
        assert!(!Ring::slot_ready(u64::MAX, 1));
        assert!(Ring::slot_ready(1, 1));
        assert_eq!(ring.select(1), (0, 0));
        ring.submitted(0, 4);
        assert_eq!(ring.select(1), (1, 2));
    }

    #[test]
    fn event_retirement_requires_the_armed_value_or_device_removal() {
        let mut armed = Some(7);
        assert!(!retire_fence_event(&mut armed, true, 3));
        assert!(!retire_fence_event(&mut armed, false, 7));
        assert_eq!(armed, Some(7));
        assert!(retire_fence_event(&mut armed, true, 7));
        assert_eq!(armed, None);

        armed = Some(9);
        assert!(retire_fence_event(&mut armed, false, u64::MAX));
        assert_eq!(armed, None);
    }

    #[test]
    fn a_retired_owner_waits_out_its_retry_delay_while_another_may_start() {
        let owner = Owner {
            device: 1,
            queue: 2,
        };
        let retry = Retry { owner, after: 44 };
        assert!(retry.allows(
            Owner {
                device: 3,
                queue: 4
            },
            43
        ));
        assert!(!retry.allows(owner, 43));
        assert!(retry.allows(owner, 44));
    }
}
