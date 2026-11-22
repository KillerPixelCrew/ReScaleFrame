// SPDX-License-Identifier: MIT
use super::{ComRef, Draw, Handle, RawPipeline, Update, Vertex};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct3D::{D3D_PRIMITIVE_TOPOLOGY, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST},
            Direct3D11::{
                D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_WRITE,
                D3D11_DEVICE_CONTEXT_IMMEDIATE, D3D11_MAP_WRITE_DISCARD, D3D11_MAPPED_SUBRESOURCE,
                D3D11_RENDER_TARGET_VIEW_DESC, D3D11_SHADER_MAX_INTERFACES, D3D11_SUBRESOURCE_DATA,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_DYNAMIC, D3D11_VIEWPORT,
                D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE, ID3D11Device,
                ID3D11DeviceChild, ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Texture2D,
            },
            Dxgi::Common::{
                DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_B8G8R8X8_UNORM_SRGB,
                DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_FORMAT_R32_UINT,
                DXGI_SAMPLE_DESC,
            },
        },
    },
    core::{BOOL, Interface},
};

pub(super) unsafe fn valid_context(device: Handle, context: Handle) -> bool {
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }) else {
        return false;
    };
    let child: &ID3D11DeviceChild = context;
    unsafe {
        context.GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE
            && child
                .GetDevice()
                .is_ok_and(|owner| owner.as_raw() == device)
    }
}

pub(super) unsafe fn texture(device: Handle, update: &Update, row_pitch: u32) -> Option<ComRef> {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }?;
    let desc = D3D11_TEXTURE2D_DESC {
        Width: update.width,
        Height: update.height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        ..Default::default()
    };
    let initial = D3D11_SUBRESOURCE_DATA {
        pSysMem: update.pixels.cast(),
        SysMemPitch: row_pitch,
        SysMemSlicePitch: 0,
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, Some(&initial), Some(&mut texture)) }.ok()?;
    unsafe { ComRef::from_owned(texture?.into_raw()) }
}

pub(super) unsafe fn view(device: Handle, texture: Handle) -> Option<ComRef> {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }?;
    let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&texture) }?;
    let mut view = None;
    unsafe { device.CreateShaderResourceView(texture, None, Some(&mut view)) }.ok()?;
    unsafe { ComRef::from_owned(view?.into_raw()) }
}

pub(super) unsafe fn patch(context: Handle, texture: Handle, update: &Update, pitch: u32) {
    let context =
        unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }.expect("live context");
    let region = D3D11_BOX {
        left: update.x,
        top: update.y,
        front: 0,
        right: update.x + update.width,
        bottom: update.y + update.height,
        back: 1,
    };
    unsafe {
        (context.vtable().UpdateSubresource)(
            context.as_raw(),
            texture,
            0,
            &region,
            update.pixels.cast(),
            pitch,
            pitch * update.height,
        );
    }
}

pub(super) unsafe fn buffer(device: Handle, bytes: u32, bind_flags: u32) -> Option<ComRef> {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }?;
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: bytes,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: bind_flags,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        ..Default::default()
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) }.ok()?;
    unsafe { ComRef::from_owned(buffer?.into_raw()) }
}

pub(super) unsafe fn write(
    context: Handle,
    buffer: Handle,
    bytes: *const c_void,
    length: usize,
) -> bool {
    let api = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }
        .expect("live context")
        .vtable();
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        if (api.Map)(context, buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, &mut mapped).is_err() {
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.cast::<u8>(), mapped.pData.cast(), length);
        (api.Unmap)(context, buffer, 0);
    }
    true
}

pub(super) unsafe fn target_kind(context: Handle) -> u32 {
    let api = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }
        .expect("live context")
        .vtable();
    let mut target = null_mut();
    unsafe { (api.OMGetRenderTargets)(context, 1, &mut target, null_mut()) };
    if target.is_null() {
        return 0;
    }
    let target = unsafe { ID3D11RenderTargetView::from_raw(target) };
    let mut desc = D3D11_RENDER_TARGET_VIEW_DESC::default();
    unsafe { target.GetDesc(&mut desc) };
    match desc.Format {
        DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
        | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
        | DXGI_FORMAT_B8G8R8X8_UNORM_SRGB => 2,
        _ => 1,
    }
}

const CLASS_INSTANCES: usize = D3D11_SHADER_MAX_INTERFACES as usize;
const VIEWPORTS: usize = D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE as usize;

struct Shader {
    shader: Option<ComRef>,
    instances: [Option<ComRef>; CLASS_INSTANCES],
    count: u32,
}

impl Default for Shader {
    fn default() -> Self {
        Self {
            shader: None,
            instances: std::array::from_fn(|_| None),
            count: CLASS_INSTANCES as u32,
        }
    }
}

fn raw(reference: &Option<ComRef>) -> Handle {
    reference.as_ref().map_or(null_mut(), ComRef::as_ptr)
}

// Option<ComRef> is a nullable interface pointer. D3D getters write one owned
// reference per slot, including class instances.
fn output(reference: &mut Option<ComRef>) -> *mut Handle {
    std::ptr::from_mut(reference).cast()
}

#[derive(Default)]
struct Snapshot {
    predicate: Option<ComRef>,
    predicate_value: BOOL,
    input_layout: Option<ComRef>,
    topology: D3D_PRIMITIVE_TOPOLOGY,
    vertex_buffer: Option<ComRef>,
    vertex_stride: u32,
    vertex_offset: u32,
    index_buffer: Option<ComRef>,
    index_format: DXGI_FORMAT,
    index_offset: u32,
    vs: Shader,
    vertex_constants: Option<ComRef>,
    ps: Shader,
    pixel_resource: Option<ComRef>,
    pixel_sampler: Option<ComRef>,
    gs: Shader,
    hs: Shader,
    ds: Shader,
    rasterizer: Option<ComRef>,
    viewports: [D3D11_VIEWPORT; VIEWPORTS],
    viewport_count: u32,
    scissors: [RECT; VIEWPORTS],
    scissor_count: u32,
    blend: Option<ComRef>,
    blend_factor: [f32; 4],
    sample_mask: u32,
    depth_stencil: Option<ComRef>,
    stencil_reference: u32,
}

impl Snapshot {
    unsafe fn capture(context: &ID3D11DeviceContext) -> Self {
        let mut state = Self {
            viewport_count: VIEWPORTS as u32,
            scissor_count: VIEWPORTS as u32,
            ..Self::default()
        };
        let ptr = context.as_raw();
        let api = context.vtable();
        unsafe {
            (api.IAGetInputLayout)(ptr, output(&mut state.input_layout));
            (api.IAGetPrimitiveTopology)(ptr, &mut state.topology);
            (api.IAGetVertexBuffers)(
                ptr,
                0,
                1,
                output(&mut state.vertex_buffer),
                &mut state.vertex_stride,
                &mut state.vertex_offset,
            );
            (api.IAGetIndexBuffer)(
                ptr,
                output(&mut state.index_buffer),
                &mut state.index_format,
                &mut state.index_offset,
            );
            (api.VSGetShader)(
                ptr,
                output(&mut state.vs.shader),
                state.vs.instances.as_mut_ptr().cast(),
                &mut state.vs.count,
            );
            (api.VSGetConstantBuffers)(ptr, 0, 1, output(&mut state.vertex_constants));
            (api.PSGetShader)(
                ptr,
                output(&mut state.ps.shader),
                state.ps.instances.as_mut_ptr().cast(),
                &mut state.ps.count,
            );
            (api.PSGetShaderResources)(ptr, 0, 1, output(&mut state.pixel_resource));
            (api.PSGetSamplers)(ptr, 0, 1, output(&mut state.pixel_sampler));
            (api.GSGetShader)(
                ptr,
                output(&mut state.gs.shader),
                state.gs.instances.as_mut_ptr().cast(),
                &mut state.gs.count,
            );
            (api.HSGetShader)(
                ptr,
                output(&mut state.hs.shader),
                state.hs.instances.as_mut_ptr().cast(),
                &mut state.hs.count,
            );
            (api.DSGetShader)(
                ptr,
                output(&mut state.ds.shader),
                state.ds.instances.as_mut_ptr().cast(),
                &mut state.ds.count,
            );
            (api.RSGetState)(ptr, output(&mut state.rasterizer));
            (api.RSGetViewports)(ptr, &mut state.viewport_count, state.viewports.as_mut_ptr());
            (api.RSGetScissorRects)(ptr, &mut state.scissor_count, state.scissors.as_mut_ptr());
            (api.OMGetBlendState)(
                ptr,
                output(&mut state.blend),
                state.blend_factor.as_mut_ptr(),
                &mut state.sample_mask,
            );
            (api.OMGetDepthStencilState)(
                ptr,
                output(&mut state.depth_stencil),
                &mut state.stencil_reference,
            );
            (api.GetPredication)(
                ptr,
                output(&mut state.predicate),
                &mut state.predicate_value,
            );
        }
        state
    }

    unsafe fn restore(self, context: &ID3D11DeviceContext) {
        let ptr = context.as_raw();
        let api = context.vtable();
        unsafe {
            (api.IASetInputLayout)(ptr, raw(&self.input_layout));
            (api.IASetPrimitiveTopology)(ptr, self.topology);
            (api.IASetVertexBuffers)(
                ptr,
                0,
                1,
                &raw(&self.vertex_buffer),
                &self.vertex_stride,
                &self.vertex_offset,
            );
            (api.IASetIndexBuffer)(
                ptr,
                raw(&self.index_buffer),
                self.index_format,
                self.index_offset,
            );
            (api.VSSetShader)(
                ptr,
                raw(&self.vs.shader),
                self.vs.instances.as_ptr().cast(),
                self.vs.count,
            );
            (api.VSSetConstantBuffers)(ptr, 0, 1, &raw(&self.vertex_constants));
            (api.PSSetShader)(
                ptr,
                raw(&self.ps.shader),
                self.ps.instances.as_ptr().cast(),
                self.ps.count,
            );
            (api.PSSetShaderResources)(ptr, 0, 1, &raw(&self.pixel_resource));
            (api.PSSetSamplers)(ptr, 0, 1, &raw(&self.pixel_sampler));
            (api.GSSetShader)(
                ptr,
                raw(&self.gs.shader),
                self.gs.instances.as_ptr().cast(),
                self.gs.count,
            );
            (api.HSSetShader)(
                ptr,
                raw(&self.hs.shader),
                self.hs.instances.as_ptr().cast(),
                self.hs.count,
            );
            (api.DSSetShader)(
                ptr,
                raw(&self.ds.shader),
                self.ds.instances.as_ptr().cast(),
                self.ds.count,
            );
            (api.RSSetState)(ptr, raw(&self.rasterizer));
            (api.RSSetViewports)(ptr, self.viewport_count, self.viewports.as_ptr());
            (api.RSSetScissorRects)(ptr, self.scissor_count, self.scissors.as_ptr());
            (api.OMSetBlendState)(
                ptr,
                raw(&self.blend),
                self.blend_factor.as_ptr(),
                self.sample_mask,
            );
            (api.OMSetDepthStencilState)(ptr, raw(&self.depth_stencil), self.stencil_reference);
            (api.SetPredication)(ptr, raw(&self.predicate), self.predicate_value);
        }
    }
}

pub(super) unsafe fn draw(
    context: Handle,
    pipeline: &RawPipeline,
    vertices: Handle,
    indices: Handle,
    [width, height]: [u32; 2],
    draws: &[Draw],
) {
    let context =
        unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }.expect("live context");
    let ptr = context.as_raw();
    let api = context.vtable();
    let viewport = D3D11_VIEWPORT {
        TopLeftX: 0.0,
        TopLeftY: 0.0,
        Width: width as f32,
        Height: height as f32,
        MinDepth: 0.0,
        MaxDepth: 1.0,
    };
    let [vs, ps, layout, constants, blend, depth, rasterizer, sampler] = *pipeline;
    unsafe {
        let saved = Snapshot::capture(context);
        (api.SetPredication)(ptr, null_mut(), BOOL(0));
        (api.IASetInputLayout)(ptr, layout);
        (api.IASetPrimitiveTopology)(ptr, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        (api.IASetVertexBuffers)(ptr, 0, 1, &vertices, &(size_of::<Vertex>() as u32), &0);
        (api.IASetIndexBuffer)(ptr, indices, DXGI_FORMAT_R32_UINT, 0);
        (api.VSSetShader)(ptr, vs, null(), 0);
        (api.VSSetConstantBuffers)(ptr, 0, 1, &constants);
        (api.PSSetShader)(ptr, ps, null(), 0);
        (api.PSSetSamplers)(ptr, 0, 1, &sampler);
        (api.GSSetShader)(ptr, null_mut(), null(), 0);
        (api.HSSetShader)(ptr, null_mut(), null(), 0);
        (api.DSSetShader)(ptr, null_mut(), null(), 0);
        (api.OMSetBlendState)(ptr, blend, [0.0f32; 4].as_ptr(), u32::MAX);
        (api.OMSetDepthStencilState)(ptr, depth, 0);
        (api.RSSetState)(ptr, rasterizer);
        (api.RSSetViewports)(ptr, 1, &viewport);
        for call in draws {
            let [left, top, right, bottom] = call.clip;
            (api.RSSetScissorRects)(
                ptr,
                1,
                &RECT {
                    left,
                    top,
                    right,
                    bottom,
                },
            );
            (api.PSSetShaderResources)(ptr, 0, 1, &call.view);
            (api.DrawIndexed)(ptr, call.index_count, call.index_offset, call.vertex_offset);
        }
        // Leave render targets, the depth view and UAVs bound for the whole pass.
        (api.PSSetShaderResources)(ptr, 0, 1, &null_mut());
        saved.restore(context);
    }
}
