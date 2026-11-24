// SPDX-License-Identifier: MIT
//! Typed D3D11 calls for the SR owners and their native fixtures. Under test the owners call the recording fakes, so
//! these are compiled but unused.
#![cfg_attr(test, allow(dead_code))]
use super::{DEPTH_SRV, DSV, Draw, Extent, Log, Ptr, ROUND_TRIP, RTV, SRV, Texture, UAV, View};
use rsf_shader_compiler::{Request, compile_system_checked};
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::{copy_nonoverlapping, null_mut};
use std::slice;
use windows::Win32::Graphics::Direct3D::{
    D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D11_SRV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::{BOOL, IUnknown, Interface};

// Texture is D3D11_TEXTURE2D_DESC field for field; describe_texture and texture transmute between them.
const _: () = {
    use std::mem::offset_of;
    type Desc = D3D11_TEXTURE2D_DESC;
    assert!(size_of::<Texture>() == size_of::<Desc>());
    assert!(offset_of!(Texture, size) == offset_of!(Desc, Width));
    assert!(offset_of!(Texture, mips) == offset_of!(Desc, MipLevels));
    assert!(offset_of!(Texture, layers) == offset_of!(Desc, ArraySize));
    assert!(offset_of!(Texture, format) == offset_of!(Desc, Format));
    assert!(offset_of!(Texture, samples) == offset_of!(Desc, SampleDesc));
    assert!(offset_of!(Texture, usage) == offset_of!(Desc, Usage));
    assert!(offset_of!(Texture, bindings) == offset_of!(Desc, BindFlags));
    assert!(offset_of!(Texture, cpu_access) == offset_of!(Desc, CPUAccessFlags));
    assert!(offset_of!(Texture, misc) == offset_of!(Desc, MiscFlags));
};

fn owned<T: Interface>(value: Option<T>) -> Ptr {
    value.map_or(null_mut(), Interface::into_raw)
}

/// The typed calls the SR owners make (`sr_d3d11::d3d`): these functions, the default shader view of the Graphics
/// leaves and the pipeline-state snapshot of sr_d3d11_platform.c.
#[cfg(not(test))]
pub(crate) mod calls {
    use super::Ptr;
    pub(crate) use super::{
        compile_with_guarded_log as shader, rsf_anomaly_sr_as_texture as as_texture,
        rsf_anomaly_sr_blend_state as blend_state, rsf_anomaly_sr_clear_stencil as clear_stencil,
        rsf_anomaly_sr_constant_buffer as constant_buffer,
        rsf_anomaly_sr_depth_state as depth_state,
        rsf_anomaly_sr_describe_texture as describe_texture,
        rsf_anomaly_sr_describe_view as describe_view, rsf_anomaly_sr_draw as draw,
        rsf_anomaly_sr_format_support as format_support, rsf_anomaly_sr_ps_constant as ps_constant,
        rsf_anomaly_sr_ps_resources as ps_resources, rsf_anomaly_sr_raster_state as raster_state,
        rsf_anomaly_sr_targets as targets, rsf_anomaly_sr_texture as texture,
        rsf_anomaly_sr_texture_device as texture_device,
        rsf_anomaly_sr_update_constants as update_constants,
        rsf_anomaly_sr_view_resource as view_resource, view_with_default_srv as view,
    };
    pub(crate) use crate::com::{release, retain};

    unsafe extern "C" {
        /// An owned snapshot of the immediate-context pipeline, or null.
        #[link_name = "rsf_anomaly_sr_save_state"]
        pub(crate) fn save_state(context: Ptr) -> Ptr;
        /// Restores the snapshot and frees it.
        #[link_name = "rsf_anomaly_sr_restore_state"]
        pub(crate) fn restore_state(context: Ptr, snapshot: Ptr);
        pub(super) fn rsf_d3d11_create_shader_view(device: Ptr, texture: Ptr) -> Ptr;
    }
}

#[cfg(not(test))]
pub(crate) unsafe fn view_with_default_srv(
    device: Ptr,
    texture: Ptr,
    kind: u32,
    source: Option<&View>,
) -> Ptr {
    if kind == SRV && source.is_none() {
        return unsafe { calls::rsf_d3d11_create_shader_view(device, texture) };
    }
    unsafe { rsf_anomaly_sr_view(device, texture, kind, source) }
}

/// Compiles one SR shader and creates it on `device`. Returns 1 with `out` set, 0 when the device refused it, or the
/// result of logging the compile refusal (0, or -1 when the guarded log contained a foreign exception).
pub(crate) unsafe fn compile_with_guarded_log(
    raw: Ptr,
    kind: u32,
    vertex: u32,
    entry: *const c_char,
    callback: Log,
    user: Ptr,
    out: &mut Ptr,
) -> i32 {
    *out = null_mut();
    let (source, name) = if kind == ROUND_TRIP {
        (
            include_bytes!("../../shaders/sr/rsf_sr_roundtrip.hlsl").as_slice(),
            c"rsf_sr_roundtrip.hlsl",
        )
    } else {
        (
            include_bytes!("../../shaders/sr/rsf_sr_upscale.hlsl").as_slice(),
            c"rsf_sr_upscale.hlsl",
        )
    };
    // Foreign log callbacks are invoked through the C++ exception boundary.
    let say = |message: String| {
        if callback.is_none() {
            return 0;
        }
        let mut message = message.into_bytes();
        message.truncate(
            message
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(message.len())
                .min(511),
        );
        let message = CString::new(message).expect("trimmed at the first NUL");
        if unsafe { rsf_game_sys::rsf_game_log_guarded(callback, user, message.as_ptr()) } {
            0
        } else {
            -1
        }
    };
    let entry = unsafe { CStr::from_ptr(entry) };
    let code = match compile_system_checked(Request {
        source,
        name,
        entry,
        target: if vertex != 0 { c"vs_5_0" } else { c"ps_5_0" },
        flags: 1 << 11, // D3DCOMPILE_ENABLE_STRICTNESS
    }) {
        Ok(code) => code,
        Err(reason) => {
            return say(format!(
                "shader {} ({}): {reason}",
                name.to_string_lossy(),
                entry.to_string_lossy()
            ));
        }
    };
    let Some(device) = (unsafe { ID3D11Device::from_raw_borrowed(&raw) }) else {
        return 0;
    };
    let mut shader = None;
    if vertex != 0 {
        if unsafe {
            device.CreateVertexShader(&code, None::<&ID3D11ClassLinkage>, Some(&mut shader))
        }
        .is_ok()
        {
            *out = owned(shader);
        }
    } else {
        let mut shader = None;
        if unsafe {
            device.CreatePixelShader(&code, None::<&ID3D11ClassLinkage>, Some(&mut shader))
        }
        .is_ok()
        {
            *out = owned(shader);
        }
    }
    i32::from(!out.is_null())
}

pub(crate) unsafe fn rsf_anomaly_sr_as_texture(raw: Ptr) -> Ptr {
    let resource = unsafe { ID3D11Resource::from_raw_borrowed(&raw) }.unwrap();
    owned(resource.cast::<ID3D11Texture2D>().ok())
}

pub(crate) unsafe fn rsf_anomaly_sr_view_resource(raw: Ptr) -> Ptr {
    let view = unsafe { ID3D11View::from_raw_borrowed(&raw) }.unwrap();
    owned(unsafe { view.GetResource() }.ok())
}

pub(crate) unsafe fn rsf_anomaly_sr_texture_device(raw: Ptr) -> Ptr {
    let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.unwrap();
    owned(unsafe { texture.GetDevice() }.ok())
}

pub(crate) unsafe fn texture_format(raw: Ptr) -> u32 {
    let Some(texture) = (unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }) else {
        return 0;
    };
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    desc.Format.0 as u32
}

pub(crate) unsafe fn rsf_anomaly_sr_describe_texture(raw: Ptr, out: &mut Texture) {
    let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.unwrap();
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    *out = unsafe {
        std::mem::transmute::<
            windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE2D_DESC,
            crate::sr_d3d11::Texture,
        >(desc)
    };
}

fn write_descriptor<D>(out: &mut View, desc: &D) {
    assert!(size_of::<D>() <= size_of::<[u32; 6]>());
    // The caller has initialized all bytes of the descriptor, including its union.
    unsafe {
        copy_nonoverlapping(
            std::ptr::from_ref(desc).cast::<u8>(),
            out.descriptor.as_mut_ptr().cast(),
            size_of::<D>(),
        )
    };
}

pub(crate) unsafe fn rsf_anomaly_sr_describe_view(raw: Ptr, kind: u32, out: &mut View) {
    macro_rules! describe {
        ($interface:ident, $desc:ident, $dimension:ident) => {{
            let view = unsafe { $interface::from_raw_borrowed(&raw) }.unwrap();
            let mut desc: $desc = unsafe { std::mem::zeroed() };
            unsafe { view.GetDesc(&mut desc) };
            out.texture2d = u32::from(desc.ViewDimension == $dimension);
            out.format = desc.Format.0 as u32;
            write_descriptor(out, &desc);
            desc
        }};
    }
    match kind {
        RTV => {
            describe!(
                ID3D11RenderTargetView,
                D3D11_RENDER_TARGET_VIEW_DESC,
                D3D11_RTV_DIMENSION_TEXTURE2D
            );
        }
        SRV => {
            describe!(
                ID3D11ShaderResourceView,
                D3D11_SHADER_RESOURCE_VIEW_DESC,
                D3D11_SRV_DIMENSION_TEXTURE2D
            );
        }
        UAV => {
            describe!(
                ID3D11UnorderedAccessView,
                D3D11_UNORDERED_ACCESS_VIEW_DESC,
                D3D11_UAV_DIMENSION_TEXTURE2D
            );
        }
        _ => {
            out.flags = describe!(
                ID3D11DepthStencilView,
                D3D11_DEPTH_STENCIL_VIEW_DESC,
                D3D11_DSV_DIMENSION_TEXTURE2D
            )
            .Flags;
        }
    }
}

pub(crate) unsafe fn rsf_anomaly_sr_format_support(raw: Ptr, format: u32) -> u32 {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    unsafe { device.CheckFormatSupport(DXGI_FORMAT(format as i32)) }.unwrap_or(0)
}

pub(crate) unsafe fn rsf_anomaly_sr_texture(
    raw: Ptr,
    input: &Texture,
    initial: *const c_void,
    pitch: u32,
) -> Ptr {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let desc: D3D11_TEXTURE2D_DESC = unsafe { std::mem::transmute(*input) };
    let data = D3D11_SUBRESOURCE_DATA {
        pSysMem: initial,
        SysMemPitch: pitch,
        SysMemSlicePitch: 0,
    };
    let mut result = None;
    let initial_data = (!initial.is_null()).then_some(std::ptr::from_ref(&data));
    if unsafe { device.CreateTexture2D(&desc, initial_data, Some(&mut result)) }.is_err() {
        return null_mut();
    }
    owned(result)
}

fn descriptor<D: Copy>(source: Option<&View>) -> Option<D> {
    assert!(size_of::<D>() <= size_of::<[u32; 6]>());
    // Only SDK view descriptors are requested, with an initialized descriptor payload.
    source.map(|source| unsafe { source.descriptor.as_ptr().cast::<D>().read_unaligned() })
}

unsafe fn rsf_anomaly_sr_view(raw: Ptr, texture: Ptr, kind: u32, source: Option<&View>) -> Ptr {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let resource = unsafe { ID3D11Resource::from_raw_borrowed(&texture) }.unwrap();
    macro_rules! create {
        ($method:ident, $desc:expr) => {{
            let desc = $desc;
            let mut result = None;
            if unsafe {
                device.$method(
                    resource,
                    desc.as_ref().map(std::ptr::from_ref),
                    Some(&mut result),
                )
            }
            .is_err()
            {
                return null_mut();
            }
            owned(result)
        }};
    }
    match kind {
        RTV => create!(CreateRenderTargetView, descriptor(source)),
        UAV => create!(CreateUnorderedAccessView, descriptor(source)),
        DSV => create!(
            CreateDepthStencilView,
            Some(D3D11_DEPTH_STENCIL_VIEW_DESC {
                Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
                ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
                ..Default::default()
            })
        ),
        DEPTH_SRV => create!(
            CreateShaderResourceView,
            Some(D3D11_SHADER_RESOURCE_VIEW_DESC {
                Format: DXGI_FORMAT_R24_UNORM_X8_TYPELESS,
                ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_SRV {
                        MostDetailedMip: 0,
                        MipLevels: 1
                    }
                },
            })
        ),
        SRV => create!(CreateShaderResourceView, descriptor(source)),
        _ => null_mut(),
    }
}

pub(crate) unsafe fn rsf_anomaly_sr_constant_buffer(raw: Ptr) -> Ptr {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: 16,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        ..Default::default()
    };
    let mut result = None;
    if unsafe { device.CreateBuffer(&desc, None, Some(&mut result)) }.is_err() {
        return null_mut();
    }
    owned(result)
}

pub(crate) unsafe fn rsf_anomaly_sr_raster_state(raw: Ptr) -> Ptr {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let desc = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_NONE,
        DepthClipEnable: true.into(),
        ..Default::default()
    };
    let mut result = None;
    if unsafe { device.CreateRasterizerState(&desc, Some(&mut result)) }.is_err() {
        return null_mut();
    }
    owned(result)
}

pub(crate) unsafe fn rsf_anomaly_sr_blend_state(raw: Ptr) -> Ptr {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let desc = {
        let mut desc = D3D11_BLEND_DESC::default();
        desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
        desc
    };
    let mut result = None;
    if unsafe { device.CreateBlendState(&desc, Some(&mut result)) }.is_err() {
        return null_mut();
    }
    owned(result)
}

pub(crate) unsafe fn rsf_anomaly_sr_depth_state(raw: Ptr, write_depth: u32) -> Ptr {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let desc = D3D11_DEPTH_STENCIL_DESC {
        DepthEnable: (write_depth != 0).into(),
        DepthWriteMask: if write_depth != 0 {
            D3D11_DEPTH_WRITE_MASK_ALL
        } else {
            D3D11_DEPTH_WRITE_MASK_ZERO
        },
        DepthFunc: D3D11_COMPARISON_ALWAYS,
        ..Default::default()
    };
    let mut result = None;
    if unsafe { device.CreateDepthStencilState(&desc, Some(&mut result)) }.is_err() {
        return null_mut();
    }
    owned(result)
}

pub(crate) unsafe fn rsf_anomaly_sr_update_constants(raw: Ptr, buffer: Ptr, data: *const u32) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let buffer = unsafe { ID3D11Buffer::from_raw_borrowed(&buffer) }.unwrap();
    unsafe { context.UpdateSubresource(buffer, 0, None, data.cast(), 0, 0) };
}

pub(crate) unsafe fn rsf_anomaly_sr_ps_constant(raw: Ptr, slot: u32, buffer: Ptr) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    // The interface option has the same ABI as this nullable COM pointer.
    unsafe {
        context.PSSetConstantBuffers(
            slot,
            Some(slice::from_raw_parts(std::ptr::from_ref(&buffer).cast(), 1)),
        )
    };
}

pub(crate) unsafe fn rsf_anomaly_sr_targets(raw: Ptr, count: u32, targets: *const Ptr, depth: Ptr) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let depth = unsafe { ID3D11DepthStencilView::from_raw_borrowed(&depth) };
    // The pass always supplies a non-null pointer to its nullable RTV array.
    let targets = unsafe { slice::from_raw_parts(targets.cast(), count as usize) };
    unsafe {
        context.OMSetRenderTargetsAndUnorderedAccessViews(Some(targets), depth, 0, 0, None, None)
    };
}

pub(crate) unsafe fn rsf_anomaly_sr_ps_resources(
    raw: Ptr,
    slot: u32,
    count: u32,
    inputs: *const Ptr,
) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let inputs = unsafe { slice::from_raw_parts(inputs.cast(), count as usize) };
    unsafe { context.PSSetShaderResources(slot, Some(inputs)) };
}

pub(crate) unsafe fn rsf_anomaly_sr_clear_stencil(raw: Ptr, depth: Ptr) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let depth = unsafe { ID3D11DepthStencilView::from_raw_borrowed(&depth) }.unwrap();
    unsafe { context.ClearDepthStencilView(depth, D3D11_CLEAR_STENCIL.0, 1.0, 0) };
}

fn without_predication(context: &ID3D11DeviceContext, operation: impl FnOnce()) {
    let mut predicate = None;
    let mut value = BOOL(0);
    unsafe {
        context.GetPredication(Some(&mut predicate), Some(&mut value));
        context.SetPredication(None::<&ID3D11Predicate>, false);
        operation();
        let predicate = predicate.as_ref().map_or(null_mut(), Interface::as_raw);
        (context.vtable().SetPredication)(context.as_raw(), predicate, value);
    }
}

fn viewport(context: &ID3D11DeviceContext, size: Extent) {
    let value = D3D11_VIEWPORT {
        Width: size.width as f32,
        Height: size.height as f32,
        MaxDepth: 1.0,
        ..Default::default()
    };
    unsafe { context.RSSetViewports(Some(&[value])) };
}

pub(crate) unsafe fn rsf_anomaly_sr_set_full_viewport(raw: Ptr, width: u32, height: u32) {
    if let Some(context) = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }
        && width != 0
        && height != 0
    {
        viewport(context, Extent { width, height });
    }
}

pub(crate) unsafe fn rsf_anomaly_sr_draw(raw: Ptr, draw: &Draw) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let vertex = unsafe { ID3D11VertexShader::from_raw_borrowed(&draw.vertex) };
    let pixel = unsafe { ID3D11PixelShader::from_raw_borrowed(&draw.pixel) };
    let raster = unsafe { ID3D11RasterizerState::from_raw_borrowed(&draw.raster) };
    let blend = unsafe { ID3D11BlendState::from_raw_borrowed(&draw.blend) };
    let depth = unsafe { ID3D11DepthStencilState::from_raw_borrowed(&draw.depth) };
    unsafe {
        context.IASetInputLayout(None::<&ID3D11InputLayout>);
        context.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        context.VSSetShader(vertex, None);
        context.HSSetShader(None::<&ID3D11HullShader>, None);
        context.DSSetShader(None::<&ID3D11DomainShader>, None);
        context.GSSetShader(None::<&ID3D11GeometryShader>, None);
        context.PSSetShader(pixel, None);
        rsf_anomaly_sr_ps_constant(raw, 0, draw.constants);
        context.RSSetState(raster);
        viewport(context, draw.size);
        context.OMSetBlendState(blend, Some(&[0.0; 4]), u32::MAX);
        context.OMSetDepthStencilState(depth, 0);
        without_predication(context, || context.Draw(3, 0));
    }
}

pub(crate) unsafe fn rsf_anomaly_sr_timer_retain_device(raw: Ptr) -> Ptr {
    unsafe { crate::com::retain(raw) };
    raw
}

pub(crate) unsafe fn rsf_anomaly_sr_timer_create_query(raw: Ptr, disjoint: bool) -> Ptr {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let desc = D3D11_QUERY_DESC {
        Query: if disjoint {
            D3D11_QUERY_TIMESTAMP_DISJOINT
        } else {
            D3D11_QUERY_TIMESTAMP
        },
        MiscFlags: 0,
    };
    let mut query = None;
    if unsafe { device.CreateQuery(&desc, Some(&mut query)) }.is_err() {
        return null_mut();
    }
    owned(query)
}

pub(crate) unsafe fn rsf_anomaly_sr_timer_issue(raw: Ptr, query: Ptr, begin: bool) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let query = unsafe { ID3D11Query::from_raw_borrowed(&query) }.unwrap();
    unsafe {
        if begin {
            context.Begin(query)
        } else {
            context.End(query)
        };
    }
}

unsafe fn query_data<T>(raw: Ptr, query: Ptr, data: *mut T) -> i32 {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    // The convenience method maps S_FALSE to Ok, so use the typed vtable slot
    // to keep pending samples distinct from ready data and terminal failures.
    unsafe {
        (context.vtable().GetData)(
            raw,
            query,
            data.cast(),
            size_of::<T>() as u32,
            D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32,
        )
        .0
    }
}

pub(crate) unsafe fn rsf_anomaly_sr_timer_clock(
    raw: Ptr,
    query: Ptr,
    frequency: *mut u64,
    disjoint: *mut bool,
) -> i32 {
    let mut data = D3D11_QUERY_DATA_TIMESTAMP_DISJOINT::default();
    let result = unsafe { query_data(raw, query, &mut data) };
    if result == 0 {
        unsafe {
            frequency.write(data.Frequency);
            disjoint.write(data.Disjoint.as_bool());
        }
    }
    result
}

pub(crate) unsafe fn rsf_anomaly_sr_timer_timestamp(raw: Ptr, query: Ptr, ticks: *mut u64) -> i32 {
    unsafe { query_data(raw, query, ticks) }
}

pub(crate) unsafe fn rsf_anomaly_sr_immediate_context(raw: Ptr) -> bool {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    unsafe { context.GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE }
}

pub(crate) unsafe fn rsf_anomaly_sr_context_device(raw: Ptr) -> Ptr {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    owned(unsafe { context.GetDevice() }.ok())
}

pub(crate) unsafe fn rsf_anomaly_sr_copy_subresource(
    raw: Ptr,
    destination: Ptr,
    dst_index: u32,
    source: Ptr,
    src_index: u32,
) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let destination = unsafe { ID3D11Resource::from_raw_borrowed(&destination) }.unwrap();
    let source = unsafe { ID3D11Resource::from_raw_borrowed(&source) }.unwrap();
    without_predication(context, || unsafe {
        context.CopySubresourceRegion(destination, dst_index, 0, 0, 0, source, src_index, None)
    });
}

pub(crate) unsafe fn rsf_anomaly_sr_map_read(
    raw: Ptr,
    texture: Ptr,
    subresource: u32,
    data: *mut *const c_void,
    row_pitch: *mut u32,
) -> bool {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let texture = unsafe { ID3D11Resource::from_raw_borrowed(&texture) }.unwrap();
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    if unsafe { context.Map(texture, subresource, D3D11_MAP_READ, 0, Some(&mut mapped)) }.is_err() {
        return false;
    }
    unsafe {
        data.write(mapped.pData.cast_const());
        row_pitch.write(mapped.RowPitch);
    }
    true
}

pub(crate) unsafe fn rsf_anomaly_sr_unmap(raw: Ptr, texture: Ptr, subresource: u32) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let texture = unsafe { ID3D11Resource::from_raw_borrowed(&texture) }.unwrap();
    unsafe { context.Unmap(texture, subresource) };
}

pub(crate) unsafe extern "C" fn rsf_anomaly_sr_twin_refs(raw: Ptr) -> u32 {
    let object = unsafe { IUnknown::from_raw_borrowed(&raw) }.unwrap();
    unsafe {
        (object.vtable().AddRef)(raw);
        (object.vtable().Release)(raw) - 1
    }
}
