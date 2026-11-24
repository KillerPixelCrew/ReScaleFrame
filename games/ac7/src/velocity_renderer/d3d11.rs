// SPDX-License-Identifier: MIT
//! AC7 stream-output capture, history accumulation and motion/depth submission.
use crate::sys::rsf_frame_tap_geometry as Geometry;
use crate::sys::{rsf_d3d11_state, rsf_d3d11_state_restore, rsf_d3d11_state_save};
use crate::velocity_commit::*;
use crate::velocity_device::{Device, Kernels};
use crate::velocity_history::*;
use crate::velocity_pass_cache::VariantRequest;
use crate::velocity_stream::*;
use rsf_game_sys::{RSF_GAME_RENDER_RESET, rsf_game_render_pass as Pass};
use std::ffi::c_void;
use std::ptr::{from_ref, null_mut};
use std::sync::OnceLock;
use windows::Win32::Graphics::{Direct3D::*, Direct3D11::*, Dxgi::Common::*};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::core::{BOOL, Interface, Result};

fn create<T>(call: impl FnOnce(*mut Option<T>) -> Result<()>) -> Option<T> {
    let mut value = None;
    call(&mut value).ok()?;
    value
}

fn raw<T: Interface>(value: &Option<T>) -> *mut c_void {
    value.as_ref().map_or(null_mut(), Interface::as_raw)
}

struct Saved<'a> {
    context: &'a ID3D11DeviceContext,
    state: &'a mut rsf_d3d11_state,
}

impl<'a> Saved<'a> {
    fn new(context: &'a ID3D11DeviceContext, state: &'a mut rsf_d3d11_state) -> Option<Self> {
        // The shared state owner keeps the COM references and handles nested saves.
        (unsafe { rsf_d3d11_state_save(context.as_raw(), state) } != 0)
            .then(|| Self { context, state })
    }
}

impl Drop for Saved<'_> {
    fn drop(&mut self) {
        unsafe { rsf_d3d11_state_restore(self.context.as_raw(), self.state) };
    }
}

struct Buffer {
    buffer: ID3D11Buffer,
    view: ID3D11ShaderResourceView,
    count: u32,
}

struct Writable {
    read: Buffer,
    write: ID3D11UnorderedAccessView,
}

fn srv_desc(records: u32) -> D3D11_SHADER_RESOURCE_VIEW_DESC {
    D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32_TYPELESS,
        ViewDimension: D3D_SRV_DIMENSION_BUFFEREX,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            BufferEx: D3D11_BUFFEREX_SRV {
                FirstElement: 0,
                NumElements: records * 16,
                Flags: D3D11_BUFFEREX_SRV_FLAG_RAW.0 as u32,
            },
        },
    }
}

fn writable(device: &ID3D11Device, records: u32, indirect: bool) -> Option<Writable> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: records.checked_mul(64).filter(|&bytes| bytes != 0)?,
        BindFlags: (D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS).0 as u32,
        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS.0 as u32
            | if indirect {
                D3D11_RESOURCE_MISC_DRAWINDIRECT_ARGS.0 as u32
            } else {
                0
            },
        ..Default::default()
    };
    let mut buffer = None;
    if let Err(error) = unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) } {
        if !indirect {
            let reason = std::ffi::CString::new(format!(
                "buffer flags={} misc={} bytes={} failed {:08x}",
                desc.BindFlags,
                desc.MiscFlags,
                desc.ByteWidth,
                error.code().0 as u32
            ))
            .ok()?;
            refused(&reason);
        }
        return None;
    }
    let buffer = buffer?;
    let uav_desc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
        Format: DXGI_FORMAT_R32_TYPELESS,
        ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
        Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
            Buffer: D3D11_BUFFER_UAV {
                FirstElement: 0,
                NumElements: records * 16,
                Flags: D3D11_BUFFER_UAV_FLAG_RAW.0 as u32,
            },
        },
    };
    let srv_desc = srv_desc(records);
    let srv = |out| unsafe { device.CreateShaderResourceView(&buffer, Some(&srv_desc), Some(out)) };
    let uav =
        |out| unsafe { device.CreateUnorderedAccessView(&buffer, Some(&uav_desc), Some(out)) };
    let (view, write);
    if indirect {
        write = create(uav)?;
        view = create(srv)?;
    } else {
        view = create(srv)?;
        write = create(uav)?;
    }
    Some(Writable {
        read: Buffer {
            buffer,
            view,
            count: records,
        },
        write,
    })
}

fn capture(device: &ID3D11Device, count: u32) -> Option<Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: count.checked_mul(64)?,
        BindFlags: (D3D11_BIND_STREAM_OUTPUT | D3D11_BIND_SHADER_RESOURCE).0 as u32,
        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS.0 as u32,
        ..Default::default()
    };
    let buffer = create(|out| unsafe { device.CreateBuffer(&desc, None, Some(out)) })?;
    let view = create(|out| unsafe {
        device.CreateShaderResourceView(&buffer, Some(&srv_desc(count)), Some(out))
    })?;
    Some(Buffer {
        buffer,
        view,
        count,
    })
}

fn upload<T>(context: &ID3D11DeviceContext, buffer: &ID3D11Buffer, value: &T) {
    unsafe { context.UpdateSubresource(buffer, 0, None, from_ref(value).cast(), 0, 0) }
}

#[derive(Default)]
struct Model {
    previous: Option<Writable>,
    now: Option<Writable>,
}

struct View {
    native_depth: ID3D11Texture2D,
    depth: ID3D11Texture2D,
    depth_target: ID3D11DepthStencilView,
    // The motion target and view hold the motion texture.
    motion_target: ID3D11RenderTargetView,
    motion_view: ID3D11ShaderResourceView,
    merged: ID3D11Texture2D,
    merged_target: ID3D11RenderTargetView,
}

impl View {
    fn new(
        device: &ID3D11Device,
        native_depth: ID3D11Texture2D,
        native_dsv: &ID3D11DepthStencilView,
        mut desc: D3D11_TEXTURE2D_DESC,
    ) -> Option<Self> {
        desc.Usage = D3D11_USAGE_DEFAULT;
        desc.CPUAccessFlags = 0;
        desc.MiscFlags = 0;
        desc.BindFlags = (D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE).0 as u32;
        let mut dsv = D3D11_DEPTH_STENCIL_VIEW_DESC::default();
        unsafe { native_dsv.GetDesc(&mut dsv) };
        dsv.Flags = 0;
        let depth = create(|out| unsafe { device.CreateTexture2D(&desc, None, Some(out)) })?;
        let depth_target =
            create(|out| unsafe { device.CreateDepthStencilView(&depth, Some(&dsv), Some(out)) })?;
        desc.Format = DXGI_FORMAT_R16G16_UNORM;
        desc.BindFlags = (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32;
        let motion = create(|out| unsafe { device.CreateTexture2D(&desc, None, Some(out)) })?;
        let motion_target =
            create(|out| unsafe { device.CreateRenderTargetView(&motion, None, Some(out)) })?;
        let motion_view =
            create(|out| unsafe { device.CreateShaderResourceView(&motion, None, Some(out)) })?;
        let merged = create(|out| unsafe { device.CreateTexture2D(&desc, None, Some(out)) })?;
        let merged_target =
            create(|out| unsafe { device.CreateRenderTargetView(&merged, None, Some(out)) })?;
        Some(Self {
            native_depth,
            depth,
            depth_target,
            motion_target,
            motion_view,
            merged,
            merged_target,
        })
    }
}

fn slot<T: Default>(entries: &mut Vec<T>, slot: u32) -> Option<&mut T> {
    let slot = slot as usize;
    if slot >= entries.len() {
        entries.try_reserve(slot + 1 - entries.len()).ok()?;
        entries.resize_with(slot + 1, T::default);
    }
    entries.get_mut(slot)
}

pub(super) struct Renderer {
    device: ID3D11Device,
    shaders: Device,
    parameters: ID3D11Buffer,
    hash_parameters: ID3D11Buffer,
    indirect: Writable,
    depth_state: ID3D11DepthStencilState,
    blend: ID3D11BlendState,
    raster: ID3D11RasterizerState,
    stream: Option<Writable>,
    captured: Option<Buffer>,
    models: Vec<Model>,
    views: Vec<Option<View>>,
}

impl Renderer {
    pub(super) fn new(device: &ID3D11Device) -> Option<Self> {
        let shaders = Device::new(device)?;
        let constant = |size| {
            let desc = D3D11_BUFFER_DESC {
                ByteWidth: size,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                ..Default::default()
            };
            create(|out| unsafe { device.CreateBuffer(&desc, None, Some(out)) })
        };
        let parameters = constant(size_of::<DrawParameters>() as u32)?;
        let hash_parameters = constant(16)?;
        let indirect = writable(device, 1, true)?;
        let depth_desc = D3D11_DEPTH_STENCIL_DESC {
            DepthEnable: true.into(),
            DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL,
            DepthFunc: D3D11_COMPARISON_GREATER_EQUAL,
            ..Default::default()
        };
        let depth_state =
            create(|out| unsafe { device.CreateDepthStencilState(&depth_desc, Some(out)) })?;
        let mut blend_desc = D3D11_BLEND_DESC::default();
        blend_desc.RenderTarget[0].RenderTargetWriteMask = 15;
        let blend = create(|out| unsafe { device.CreateBlendState(&blend_desc, Some(out)) })?;
        let raster_desc = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            DepthClipEnable: true.into(),
            ..Default::default()
        };
        let raster =
            create(|out| unsafe { device.CreateRasterizerState(&raster_desc, Some(out)) })?;
        Some(Self {
            device: device.clone(),
            shaders,
            parameters,
            hash_parameters,
            indirect,
            depth_state,
            blend,
            raster,
            stream: None,
            captured: None,
            models: Vec::new(),
            views: Vec::new(),
        })
    }

    fn prepare_indirect(
        &self,
        c: &ID3D11DeviceContext,
        g: &Geometry,
        bounds: &IndirectBounds,
    ) -> Option<()> {
        let source = unsafe { ID3D11Buffer::from_raw_borrowed(&g.indirect_buffer) }?;
        if bounds.vertices == 0 {
            return None;
        }
        let mut desc = D3D11_BUFFER_DESC::default();
        unsafe { source.GetDesc(&mut desc) };
        let end = indirect_end(g.kind, g.indirect_offset, desc.ByteWidth)?;
        if unsafe { source.GetDevice() }.ok()?.as_raw() != self.device.as_raw() {
            return None;
        }
        let extent = D3D11_BOX {
            left: g.indirect_offset,
            top: 0,
            front: 0,
            right: end,
            bottom: 1,
            back: 1,
        };
        unsafe {
            c.CopySubresourceRegion(
                &self.indirect.read.buffer,
                0,
                0,
                0,
                0,
                source,
                0,
                Some(&extent),
            );
            upload(c, &self.hash_parameters, bounds);
            c.CSSetConstantBuffers(0, Some(&[Some(self.hash_parameters.clone())]));
            c.CSSetUnorderedAccessViews(0, 1, Some(&Some(self.indirect.write.clone())), None);
            c.CSSetShader(&self.shaders.kernels.validate_indirect, None);
            c.Dispatch(1, 1, 1);
            c.CSSetUnorderedAccessViews(0, 1, Some(&None), None);
        }
        Some(())
    }

    fn view(&mut self, c: &ID3D11DeviceContext, g: &Geometry, scope: &Pass) -> Option<u32> {
        let dsv = unsafe { ID3D11DepthStencilView::from_raw_borrowed(&g.depth_view) }?;
        let source = unsafe { dsv.GetResource() }.ok()?;
        let depth: ID3D11Texture2D = source.cast().ok()?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { depth.GetDesc(&mut desc) };
        if desc.Width != g.width
            || desc.Height != g.height
            || desc.ArraySize != 1
            || desc.MipLevels != 1
            || desc.SampleDesc.Count != 1
        {
            return None;
        }
        let draw = ViewDraw {
            session_id: scope.session_id,
            history_key: scope.history_key,
            view_key: scope.view_key,
            family_key: scope.family_key,
            frame: scope.native_frame as u32,
            width: desc.Width,
            height: desc.Height,
        };
        let decision = view_begin(&draw);
        if decision.action == ViewAction::Unavailable || decision.slot == NO_SLOT {
            return None;
        }
        let entry = slot(&mut self.views, decision.slot)?;
        if decision.action == ViewAction::SameFrame {
            return entry
                .as_ref()
                .filter(|v| v.native_depth.as_raw() == depth.as_raw())
                .map(|_| decision.slot);
        }
        if decision.action == ViewAction::Allocate {
            *entry = None;
            *entry = Some(View::new(&self.device, depth.clone(), dsv, desc)?);
        }
        let view = entry.as_mut()?;
        view.native_depth = depth;
        unsafe {
            c.CopyResource(&view.depth, &view.native_depth);
            c.ClearRenderTargetView(&view.motion_target, &[0.; 4]);
        }
        view_ready(decision.slot, &draw);
        Some(decision.slot)
    }

    pub(super) fn retire(&mut self, retired: Retired) {
        match retired {
            Retired::Model(slot) => {
                if let Some(entry) = self.models.get_mut(slot as usize) {
                    *entry = Model::default();
                }
            }
            Retired::View(slot) => {
                if let Some(entry) = self.views.get_mut(slot as usize) {
                    *entry = None;
                }
            }
        }
    }

    pub(super) fn count(&self) -> u32 {
        self.shaders.count
    }

    pub(super) fn draw(
        &mut self,
        c: &ID3D11DeviceContext,
        scope: &Pass,
        g: &Geometry,
        inputs: Pipeline,
        entered: f64,
    ) -> Option<()> {
        let mut timing = Timing::new(entered, scope.native_frame as u32);
        let mut pixel = None;
        unsafe { c.PSGetShader(&mut pixel, None, None) };
        let variant = self.shaders.variant(&VariantRequest {
            shaders: [
                g.vertex_shader,
                raw(&pixel),
                raw(&inputs.geometry),
                raw(&inputs.hull),
                raw(&inputs.domain),
            ],
            layout: g.input_layout,
            material_blend: scope.material_blend,
            stride: g.strides[0],
        });
        timing.phase = clock_ms();
        timing.sample.shader_ms = timing.phase - entered;
        let variant = variant?;
        let plan = plan(
            g,
            inputs.stages,
            scope.geometry_capacity,
            variant.maximum_vertices,
            variant.input_points,
        )?;
        let count = plan.bounds.records;
        timing.sample.records = count;
        let mut state = rsf_d3d11_state { opaque: [0; 1024] };
        let _saved = Saved::new(c, &mut state)?;
        let mut predicate = None;
        let mut predicate_value = BOOL(0);
        unsafe {
            c.GetPredication(Some(&mut predicate), Some(&mut predicate_value));
            c.SetPredication(None, false);
        }
        let indirect = g.kind >= 5;
        if indirect && self.prepare_indirect(c, g, &plan.bounds).is_none() {
            refused(c"indirect argument buffer or native mesh capacity is invalid");
            return None;
        }
        let Some(view_slot) = self.view(c, g, scope) else {
            refused(c"native scene depth allocation/identity differs from the draw");
            return None;
        };
        let view = self.views[view_slot as usize].as_ref()?;
        let draw = ModelDraw {
            session_id: scope.session_id,
            history_key: scope.history_key,
            view_key: scope.view_key,
            primitive_key: scope.primitive_key,
            material_key: scope.material_key,
            vertex_factory_key: scope.vertex_factory_key,
            vertex_hash: variant.vertex_hash,
            geometry_hash: variant.geometry_hash,
            start: g.start,
            base_vertex: g.base_vertex,
            particle: u32::from(variant.particle),
            frame: scope.native_frame as u32,
            reset: u32::from(scope.flags & RSF_GAME_RENDER_RESET != 0),
            records: count,
        };
        let hash = HashPass {
            c,
            kernels: &self.shaders.kernels,
            parameters: &self.hash_parameters,
            indirect: &self.indirect,
        };
        let Some((model, decision)) = model(&mut self.models, &self.device, &hash, &draw) else {
            refused(c"geometry-history allocation failed");
            return None;
        };
        if self
            .captured
            .as_ref()
            .is_none_or(|buffer| buffer.count < count)
        {
            let Some(buffer) = capture(&self.device, plan.capacity) else {
                refused(c"shader-readable stream-output allocation failed");
                return None;
            };
            self.captured = Some(buffer);
        }
        let captured = self.captured.as_ref()?;
        if plan.zero_source != 0 {
            if self
                .stream
                .as_ref()
                .is_none_or(|buffer| buffer.read.count < count)
            {
                let Some(buffer) = writable(&self.device, plan.capacity, false) else {
                    refused(c"generated-geometry zero allocation failed");
                    return None;
                };
                unsafe { c.ClearUnorderedAccessViewUint(&buffer.write, &[0; 4]) };
                self.stream = Some(buffer);
            }
            let extent = D3D11_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: count * 64,
                bottom: 1,
                back: 1,
            };
            unsafe {
                c.CopySubresourceRegion(
                    &captured.buffer,
                    0,
                    0,
                    0,
                    0,
                    &self.stream.as_ref()?.read.buffer,
                    0,
                    Some(&extent),
                )
            };
        }
        timing.sample.resource_ms = clock_ms() - timing.phase;
        let tess = inputs.hull.is_some();
        let cb = [Some(self.parameters.clone())];
        let predicate = (predicate.as_ref(), predicate_value.as_bool());
        unsafe {
            if inputs.geometry.is_some() || tess {
                upload(c, &self.parameters, &DrawParameters::default());
                c.GSSetConstantBuffers(13, Some(&cb));
                c.GSSetShaderResources(127, Some(&[None]));
                c.DSSetConstantBuffers(13, Some(&cb));
                c.DSSetShaderResources(127, Some(&[None]));
            }
            c.OMSetRenderTargets(None, None);
            c.VSSetShader(variant.vertex.as_ref(), None);
            if let Some(layout) = &variant.layout {
                c.IASetInputLayout(layout);
            }
            if tess {
                c.HSSetShader(variant.corners.as_ref(), None);
                c.DSSetShader(variant.domain.as_ref(), None);
            }
            c.GSSetShader(variant.capture.as_ref(), None);
            c.PSSetShader(None, None);
            c.SOSetTargets(1, Some(&Some(captured.buffer.clone())), Some(&0));
            c.SetPredication(predicate.0, predicate.1);
            submit(c, g, &self.indirect.read.buffer);
            c.SetPredication(None, false);
            c.SOSetTargets(0, None, None);
        }
        let mut viewport = D3D11_VIEWPORT::default();
        let mut viewports = 1;
        unsafe { c.RSGetViewports(&mut viewports, Some(&mut viewport)) };
        let rect = [
            viewport.TopLeftX,
            viewport.TopLeftY,
            viewport.Width,
            viewport.Height,
        ];
        let previous = model.previous.as_ref().map(|b| &b.read);
        let parameters = draw_parameters(
            scope,
            viewports,
            rect,
            decision.usable,
            previous.map_or(0, |b| b.count),
        )?;
        let previous = [previous.map(|b| b.view.clone())];
        unsafe {
            upload(c, &self.parameters, &parameters);
            c.GSSetConstantBuffers(13, Some(&cb));
            c.PSSetConstantBuffers(13, Some(&cb));
            c.GSSetShaderResources(variant.history_slot, Some(&previous));
            if tess {
                c.HSSetShader(variant.hull.as_ref(), None);
                c.DSSetShader(variant.domain.as_ref(), None);
                c.DSSetConstantBuffers(13, Some(&cb));
                c.DSSetShaderResources(127, Some(&previous));
            }
            c.GSSetShader(variant.geometry.as_ref(), None);
            c.PSSetShader(variant.pixel.as_ref(), None);
            c.OMSetRenderTargets(
                Some(&[Some(view.motion_target.clone())]),
                &view.depth_target,
            );
            c.OMSetDepthStencilState(&self.depth_state, 0);
            c.OMSetBlendState(&self.blend, None, u32::MAX);
            c.SetPredication(predicate.0, predicate.1);
            submit(c, g, &self.indirect.read.buffer);
            c.SetPredication(None, false);
            view_written(view_slot);
            c.GSSetShaderResources(variant.history_slot, Some(&[None]));
            c.OMSetRenderTargets(None, None);
            if tess {
                c.DSSetShaderResources(127, Some(&[None]));
            }
        }
        if hash
            .build(
                captured,
                model.now.as_ref()?,
                plan.built,
                0,
                indirect,
                predicate,
            )
            .is_some()
        {
            model_record(decision.slot, draw.frame, draw.records);
        }
        Some(())
    }

    pub(super) fn commit(&self, c: &ID3D11DeviceContext, pass: &Pass) -> Option<()> {
        let found = commit_view(pass)?;
        let view = self.views.get(found.slot as usize)?.as_ref()?;
        if view.native_depth.as_raw() != pass.depth {
            return None;
        }
        let native = unsafe { ID3D11Texture2D::from_raw_borrowed(&pass.motion) }?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { native.GetDesc(&mut desc) };
        let target = CommitTarget {
            width: desc.Width,
            height: desc.Height,
            samples: desc.SampleDesc.Count,
            array_size: desc.ArraySize,
            format: desc.Format.0 as u32,
        };
        if !target_matches(&found, &target) {
            return None;
        }
        let device = unsafe { c.GetDevice() }.ok()?;
        if device.as_raw() != self.device.as_raw() {
            return None;
        }
        let mut state = rsf_d3d11_state { opaque: [0; 1024] };
        let _saved = Saved::new(c, &mut state)?;
        unsafe {
            c.SetPredication(None, false);
            c.SOSetTargets(0, None, None);
            c.GSSetShader(None, None);
            c.HSSetShader(None, None);
            c.DSSetShader(None, None);
            c.VSSetShader(&self.shaders.kernels.fullscreen, None);
            c.PSSetShader(&self.shaders.kernels.merge, None);
            c.OMSetDepthStencilState(None, 0);
            c.OMSetBlendState(&self.blend, None, u32::MAX);
            c.RSSetState(&self.raster);
            c.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.,
                TopLeftY: 0.,
                Width: found.width as f32,
                Height: found.height as f32,
                MinDepth: 0.,
                MaxDepth: 1.,
            }]));
            c.IASetInputLayout(None);
            c.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        }
        let srv = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R16G16_UNORM,
            ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_SRV {
                    MostDetailedMip: 0,
                    MipLevels: 1,
                },
            },
        };
        let native_view = create(|out| unsafe {
            device.CreateShaderResourceView(native, Some(&srv), Some(out))
        })?;
        let empty = [const { None }; 128];
        unsafe {
            c.PSSetShaderResources(
                0,
                Some(&[Some(native_view.clone()), Some(view.motion_view.clone())]),
            );
            c.OMSetRenderTargets(Some(&[Some(view.merged_target.clone())]), None);
            super::replay::merge(|| c.Draw(3, 0));
            c.OMSetRenderTargets(None, None);
            c.VSSetShaderResources(0, Some(&empty));
            c.PSSetShaderResources(0, Some(&empty));
            c.GSSetShaderResources(0, Some(&empty));
            c.HSSetShaderResources(0, Some(&empty));
            c.DSSetShaderResources(0, Some(&empty));
            c.CSSetShaderResources(0, Some(&empty));
            c.CopyResource(native, &view.merged);
            c.CopyResource(&view.native_depth, &view.depth);
        }
        view_committed(found.slot);
        log_commit();
        Some(())
    }
}

struct HashPass<'a> {
    c: &'a ID3D11DeviceContext,
    kernels: &'a Kernels,
    parameters: &'a ID3D11Buffer,
    indirect: &'a Writable,
}

impl HashPass<'_> {
    fn build(
        &self,
        source: &Buffer,
        destination: &Writable,
        count: u32,
        mode: u32,
        dynamic: bool,
        predicate: (Option<&ID3D11Predicate>, bool),
    ) -> Option<()> {
        let dispatch = hash_dispatch(count, destination.read.count, mode, dynamic)?;
        let c = self.c;
        unsafe {
            upload(c, self.parameters, &dispatch.parameters);
            c.CSSetConstantBuffers(0, Some(&[Some(self.parameters.clone())]));
            c.CSSetShaderResources(
                0,
                Some(&[
                    Some(source.view.clone()),
                    dynamic.then(|| self.indirect.read.view.clone()),
                ]),
            );
            c.CSSetUnorderedAccessViews(0, 1, Some(&Some(destination.write.clone())), None);
            c.CSSetShader(&self.kernels.hash, None);
            c.SetPredication(predicate.0, predicate.1);
            if dynamic {
                c.DispatchIndirect(&self.indirect.read.buffer, 36);
            } else {
                c.Dispatch(dispatch.groups[0], dispatch.groups[1], 1);
            }
            c.SetPredication(None, false);
            c.CSSetShaderResources(0, Some(&[None, None]));
            c.CSSetUnorderedAccessViews(0, 1, Some(&None), None);
        }
        Some(())
    }
}

fn model<'a>(
    models: &'a mut Vec<Model>,
    device: &ID3D11Device,
    hash: &HashPass<'_>,
    draw: &ModelDraw,
) -> Option<(&'a Model, ModelDecision)> {
    let (admitted, decision) = model_begin(draw);
    if decision.slot == NO_SLOT {
        return None;
    }
    let model = slot(models, decision.slot)?;
    if decision.rotate {
        std::mem::swap(&mut model.previous, &mut model.now);
        if let Some(now) = &model.now {
            unsafe { hash.c.ClearUnorderedAccessViewUint(&now.write, &[0; 4]) };
        }
    }
    if !admitted {
        return None;
    }
    if model
        .now
        .as_ref()
        .is_none_or(|now| now.read.count < decision.capacity)
    {
        let larger = writable(device, decision.capacity, false)?;
        unsafe { hash.c.ClearUnorderedAccessViewUint(&larger.write, &[0; 4]) };
        if let Some(now) = &model.now {
            let _ = hash.build(&now.read, &larger, now.read.count, 1, false, (None, false));
        }
        model.now = Some(larger);
    }
    Some((model, decision))
}

pub(super) struct Pipeline {
    geometry: Option<ID3D11GeometryShader>,
    hull: Option<ID3D11HullShader>,
    domain: Option<ID3D11DomainShader>,
    stages: u32,
}

impl Pipeline {
    pub(super) fn inspect(c: &ID3D11DeviceContext, topology: u32) -> Option<Self> {
        let (mut geometry, mut hull, mut domain) = (None, None, None);
        unsafe {
            c.GSGetShader(&mut geometry, None, None);
            c.HSGetShader(&mut hull, None, None);
            c.DSGetShader(&mut domain, None, None);
        }
        let stages = (u32::from(geometry.is_some()) * STAGE_GEOMETRY)
            | (u32::from(hull.is_some()) * STAGE_HULL)
            | (u32::from(domain.is_some()) * STAGE_DOMAIN);
        let route = route(topology, stages);
        if route == ROUTE_MISMATCHED {
            refused(c"incomplete tessellation pipeline or combined native GS/tessellation requires a matching route");
        }
        (route == ROUTE_ADMITTED).then_some(Self {
            geometry,
            hull,
            domain,
            stages,
        })
    }
}

unsafe fn submit(c: &ID3D11DeviceContext, g: &Geometry, arguments: &ID3D11Buffer) {
    unsafe {
        match g.kind {
            0 => c.Draw(g.count, g.start),
            1 => c.DrawIndexed(g.count, g.start, g.base_vertex),
            2 => c.DrawInstanced(g.count, g.instances, g.start, g.start_instance),
            3 => c.DrawIndexedInstanced(
                g.count,
                g.instances,
                g.start,
                g.base_vertex,
                g.start_instance,
            ),
            4 => c.DrawAuto(),
            5 => c.DrawIndexedInstancedIndirect(arguments, 0),
            6 => c.DrawInstancedIndirect(arguments, 0),
            _ => {}
        }
    }
}

pub(super) fn clock_ms() -> f64 {
    static SCALE: OnceLock<f64> = OnceLock::new();
    let scale = SCALE.get_or_init(|| {
        let mut frequency = 0;
        let _ = unsafe { QueryPerformanceFrequency(&mut frequency) };
        1000. / frequency as f64
    });
    let mut ticks = 0;
    let _ = unsafe { QueryPerformanceCounter(&mut ticks) };
    ticks as f64 * scale
}

struct Timing {
    entered: f64,
    phase: f64,
    sample: ProfileSample,
}

impl Timing {
    fn new(entered: f64, frame: u32) -> Self {
        Self {
            entered,
            phase: entered,
            sample: ProfileSample {
                frame,
                ..Default::default()
            },
        }
    }
}

impl Drop for Timing {
    fn drop(&mut self) {
        self.sample.elapsed_ms = clock_ms() - self.entered;
        profile_sample(&self.sample);
    }
}
