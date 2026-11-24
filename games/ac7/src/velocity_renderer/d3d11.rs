// SPDX-License-Identifier: MIT
//! AC7 stream-output capture, history accumulation and motion/depth submission.
use crate::sys::rsf_frame_tap_geometry as Geometry;
use crate::sys::{rsf_d3d11_state, rsf_d3d11_state_restore, rsf_d3d11_state_save};
use crate::velocity_commit::*;
use crate::velocity_device::{Device, Kernels};
use crate::velocity_pass_cache::VariantRequest;
use crate::velocity_stream::*;
use rsf_game_sys::{RSF_GAME_RENDER_RESET, rsf_game_render_pass as Pass};
use std::collections::HashMap;
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

const NO_FRAME: u32 = u32::MAX;
// An entry survives the frame end it was used in and the one after.
const RETAINED_EPOCHS: u64 = 2;

/// The session and the history key, or the view key when the scope has none.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ViewKey(u64, u64);

impl ViewKey {
    fn of(pass: &Pass) -> Self {
        let key = if pass.history_key != 0 {
            pass.history_key
        } else {
            pass.view_key
        };
        Self(pass.session_id, key)
    }
}

/// A draw's geometry identity. Particles drop the vertex factory and the draw offsets.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ModelKey {
    view: ViewKey,
    primitive: u64,
    material: u64,
    vertex_hash: u32,
    vertex_factory: u64,
    start: u32,
    base_vertex: i32,
    geometry_hash: u32,
}

/// One model's GPU history: last frame's records and this frame's, the native frame they belong
/// to, this frame's record count and the frame-end epoch it was last used in.
struct Model {
    previous: Option<Writable>,
    now: Option<Writable>,
    frame: u32,
    records: u32,
    epoch: u64,
    usable: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            previous: None,
            now: None,
            frame: NO_FRAME,
            records: 0,
            epoch: 0,
            usable: false,
        }
    }
}

/// The owned depth, translucent motion and merge targets of one view.
struct Targets {
    native_depth: ID3D11Texture2D,
    depth: ID3D11Texture2D,
    depth_target: ID3D11DepthStencilView,
    // The motion target and view hold the motion texture.
    motion_target: ID3D11RenderTargetView,
    motion_view: ID3D11ShaderResourceView,
    merged: ID3D11Texture2D,
    merged_target: ID3D11RenderTargetView,
}

impl Targets {
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

/// One view: the native frame, view and family its targets were last prepared for, their extent,
/// whether translucent motion was written since, and the frame-end epoch it was last used in.
struct View {
    targets: Option<Targets>,
    frame: u32,
    width: u32,
    height: u32,
    family: u64,
    view: u64,
    epoch: u64,
    written: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            targets: None,
            frame: NO_FRAME,
            width: 0,
            height: 0,
            family: 0,
            view: 0,
            epoch: 0,
            written: false,
        }
    }
}

impl View {
    /// Prepared for this pass's frame, view and family.
    fn matches(&self, pass: &Pass) -> bool {
        self.frame == pass.native_frame as u32
            && self.view == pass.view_key
            && self.family == pass.family_key
    }
}

pub(super) struct Renderer {
    pub(super) device: ID3D11Device,
    shaders: Device,
    parameters: ID3D11Buffer,
    hash_parameters: ID3D11Buffer,
    indirect: Writable,
    depth_state: ID3D11DepthStencilState,
    blend: ID3D11BlendState,
    raster: ID3D11RasterizerState,
    stream: Option<Writable>,
    captured: Option<Buffer>,
    models: HashMap<ModelKey, Model>,
    views: HashMap<ViewKey, View>,
    epoch: u64,
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
            models: HashMap::new(),
            views: HashMap::new(),
            epoch: 1,
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

    /// The draw's view with its targets prepared for this native frame: allocated at the native
    /// depth's extent, the depth copied and the motion cleared at the frame's first draw. A later
    /// draw of the same frame needs the same view, family, extent and native depth.
    fn view(&mut self, c: &ID3D11DeviceContext, g: &Geometry, scope: &Pass) -> Option<ViewKey> {
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
        let key = ViewKey::of(scope);
        self.views.try_reserve(1).ok()?;
        let view = self.views.entry(key).or_default();
        let extent = view.width == desc.Width && view.height == desc.Height;
        if let Some(targets) = &view.targets
            && view.frame == scope.native_frame as u32
        {
            return (view.matches(scope)
                && extent
                && targets.native_depth.as_raw() == depth.as_raw())
            .then_some(key);
        }
        if view.targets.is_none() || !extent {
            // The old targets go first; the view stays unprepared until every target exists.
            *view = View {
                width: desc.Width,
                height: desc.Height,
                ..View::default()
            };
            view.targets = Some(Targets::new(&self.device, depth.clone(), dsv, desc)?);
        }
        let targets = view.targets.as_mut()?;
        targets.native_depth = depth;
        unsafe {
            c.CopyResource(&targets.depth, &targets.native_depth);
            c.ClearRenderTargetView(&targets.motion_target, &[0.; 4]);
        }
        view.frame = scope.native_frame as u32;
        view.family = scope.family_key;
        view.view = scope.view_key;
        view.epoch = self.epoch;
        view.written = false;
        Some(key)
    }

    /// Advances the epoch and releases the models and views not used in the last two frames.
    pub(super) fn frame_end(&mut self) {
        log_frame_end(self.count(), self.models.len());
        self.epoch += 1;
        let epoch = self.epoch;
        self.models
            .retain(|_, model| model.epoch + RETAINED_EPOCHS >= epoch);
        self.views
            .retain(|_, view| view.epoch + RETAINED_EPOCHS >= epoch);
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
        let Some(view_key) = self.view(c, g, scope) else {
            refused(c"native scene depth allocation/identity differs from the draw");
            return None;
        };
        let view = self.views.get(&view_key)?;
        let targets = view.targets.as_ref()?;
        let particle = variant.particle;
        let key = ModelKey {
            view: view_key,
            primitive: scope.primitive_key,
            material: scope.material_key,
            vertex_hash: variant.vertex_hash,
            vertex_factory: if particle {
                0
            } else {
                scope.vertex_factory_key
            },
            start: if particle { 0 } else { g.start },
            base_vertex: if particle { 0 } else { g.base_vertex },
            geometry_hash: variant.geometry_hash,
        };
        let hash = HashPass {
            c,
            kernels: &self.shaders.kernels,
            parameters: &self.hash_parameters,
            indirect: &self.indirect,
        };
        let frame = scope.native_frame as u32;
        let reset = scope.flags & RSF_GAME_RENDER_RESET != 0;
        let Some(model) = model(
            &mut self.models,
            self.epoch,
            &self.device,
            &hash,
            key,
            frame,
            reset,
            count,
        ) else {
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
        if plan.zero_source {
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
            model.usable,
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
                Some(&[Some(targets.motion_target.clone())]),
                &targets.depth_target,
            );
            c.OMSetDepthStencilState(&self.depth_state, 0);
            c.OMSetBlendState(&self.blend, None, u32::MAX);
            c.SetPredication(predicate.0, predicate.1);
            submit(c, g, &self.indirect.read.buffer);
            c.SetPredication(None, false);
            c.GSSetShaderResources(variant.history_slot, Some(&[None]));
            c.OMSetRenderTargets(None, None);
            if tess {
                c.DSSetShaderResources(127, Some(&[None]));
            }
        }
        if let Some(view) = self.views.get_mut(&view_key) {
            view.written = true;
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
            && count <= RECORD_LIMIT - model.records
        {
            // Charged only once the native build ran, so a refused draw leaves no records.
            model.records += count;
        }
        Some(())
    }

    /// Merges the view's translucent motion into the native motion target and puts its depth
    /// back, when the pass leases both and the view has motion written for its frame, view and
    /// family on the same native depth.
    pub(super) fn commit(&mut self, c: &ID3D11DeviceContext, pass: &Pass) -> Option<()> {
        if pass.motion.is_null() || pass.depth.is_null() {
            return None;
        }
        let view_key = ViewKey::of(pass);
        let view = self.views.get(&view_key)?;
        let targets = view.targets.as_ref()?;
        if !view.written || !view.matches(pass) || targets.native_depth.as_raw() != pass.depth {
            return None;
        }
        let native = unsafe { ID3D11Texture2D::from_raw_borrowed(&pass.motion) }?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { native.GetDesc(&mut desc) };
        if desc.Width != view.width
            || desc.Height != view.height
            || desc.SampleDesc.Count != 1
            || desc.ArraySize != 1
            || !matches!(
                desc.Format,
                DXGI_FORMAT_R16G16_UNORM | DXGI_FORMAT_R16G16_TYPELESS
            )
        {
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
                Width: view.width as f32,
                Height: view.height as f32,
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
                Some(&[Some(native_view.clone()), Some(targets.motion_view.clone())]),
            );
            c.OMSetRenderTargets(Some(&[Some(targets.merged_target.clone())]), None);
            super::replay::merge(|| c.Draw(3, 0));
            c.OMSetRenderTargets(None, None);
            c.VSSetShaderResources(0, Some(&empty));
            c.PSSetShaderResources(0, Some(&empty));
            c.GSSetShaderResources(0, Some(&empty));
            c.HSSetShaderResources(0, Some(&empty));
            c.DSSetShaderResources(0, Some(&empty));
            c.CSSetShaderResources(0, Some(&empty));
            c.CopyResource(native, &targets.merged);
            c.CopyResource(&targets.native_depth, &targets.depth);
        }
        if let Some(view) = self.views.get_mut(&view_key) {
            view.written = false;
        }
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

/// The model's history for this draw. At the first draw of a native frame it rotates: history is
/// usable only for the next native frame without a reset, and the new current buffer is cleared.
/// The epoch is refreshed even when the record count is then refused. The current buffer grows to
/// twice the frame's records, keeping what it already holds.
#[allow(clippy::too_many_arguments)]
fn model<'a>(
    models: &'a mut HashMap<ModelKey, Model>,
    epoch: u64,
    device: &ID3D11Device,
    hash: &HashPass<'_>,
    key: ModelKey,
    frame: u32,
    reset: bool,
    count: u32,
) -> Option<&'a mut Model> {
    models.try_reserve(1).ok()?;
    let model = models.entry(key).or_default();
    if model.frame != frame {
        model.usable = model.frame != NO_FRAME && model.frame.wrapping_add(1) == frame && !reset;
        model.frame = frame;
        model.records = 0;
        std::mem::swap(&mut model.previous, &mut model.now);
        if let Some(now) = &model.now {
            unsafe { hash.c.ClearUnorderedAccessViewUint(&now.write, &[0; 4]) };
        }
    }
    model.epoch = epoch;
    if count > u32::MAX - model.records || model.records + count > RECORD_LIMIT {
        return None;
    }
    let mut capacity = MIN_CAPACITY;
    while capacity < (model.records + count) * 2 {
        if capacity > u32::MAX / 2 {
            return None;
        }
        capacity *= 2;
    }
    if model
        .now
        .as_ref()
        .is_none_or(|now| now.read.count < capacity)
    {
        let larger = writable(device, capacity, false)?;
        unsafe { hash.c.ClearUnorderedAccessViewUint(&larger.write, &[0; 4]) };
        if let Some(now) = &model.now {
            let _ = hash.build(&now.read, &larger, now.read.count, 1, false, (None, false));
        }
        model.now = Some(larger);
    }
    Some(model)
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
        if route == Route::Mismatched {
            refused(c"incomplete tessellation pipeline or combined native GS/tessellation requires a matching route");
        }
        (route == Route::Admitted).then_some(Self {
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
