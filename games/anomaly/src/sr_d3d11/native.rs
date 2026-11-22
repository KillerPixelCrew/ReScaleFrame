// SPDX-License-Identifier: MIT
//! Typed D3D11 calls for the SR owners and their native fixtures.
use super::{DEPTH_SRV, DSV, Draw, Extent, Ptr, ROUND_TRIP, RTV, SRV, Texture, UAV, View};
use rsf_shader_compiler::{Compiler, Request, compile};
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::{copy_nonoverlapping, null_mut};
use std::slice;
use windows::Win32::Graphics::Direct3D::{
    D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D11_SRV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::{BOOL, IUnknown, Interface};

fn owned<T: Interface>(value: Option<T>) -> Ptr {
    value.map_or(null_mut(), Interface::into_raw)
}

// Foreign log callbacks are invoked through the C++ exception boundary.
type ShaderLog = Option<unsafe extern "C" fn(Ptr, *const c_char) -> bool>;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_compile_shader(
    raw: Ptr,
    kind: u32,
    vertex: u32,
    entry: *const c_char,
    log: ShaderLog,
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
    let say = |message: String| {
        let Some(emit) = log else { return 0 };
        let mut message = message.into_bytes();
        message.truncate(
            message
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(message.len())
                .min(511),
        );
        let message = CString::new(message).expect("trimmed at the first NUL");
        if unsafe { emit(user, message.as_ptr()) } {
            0
        } else {
            -1
        }
    };
    let Some(compiler) = Compiler::system("d3dcompiler_47.dll") else {
        return say(format!(
            "shader {}: d3dcompiler_47 is not available",
            name.to_string_lossy()
        ));
    };
    let Some(address) = compiler.entry() else {
        return 0;
    };
    let entry = unsafe { CStr::from_ptr(entry) };
    let result = unsafe {
        compile(
            address,
            Request {
                source,
                name,
                entry,
                target: if vertex != 0 { c"vs_5_0" } else { c"ps_5_0" },
                flags: 1 << 11, // D3DCOMPILE_ENABLE_STRICTNESS
            },
            &[],
            null_mut(),
        )
    };
    let Some(code) = result.code.filter(|_| result.status >= 0) else {
        let status = if result.status < 0 {
            result.status as u32
        } else {
            0x8000_4005
        };
        let details = result.errors.as_deref().map(String::from_utf8_lossy);
        return say(format!(
            "shader {} ({}): compile failed 0x{status:08x} {}",
            name.to_string_lossy(),
            entry.to_string_lossy(),
            details.as_deref().unwrap_or_default()
        ));
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_retain(raw: Ptr) {
    let object = unsafe { IUnknown::from_raw_borrowed(&raw) }.unwrap();
    unsafe { (object.vtable().AddRef)(raw) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_release(raw: Ptr) {
    if !raw.is_null() {
        drop(unsafe { IUnknown::from_raw(raw) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_as_texture(raw: Ptr) -> Ptr {
    let resource = unsafe { ID3D11Resource::from_raw_borrowed(&raw) }.unwrap();
    owned(resource.cast::<ID3D11Texture2D>().ok())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_view_resource(raw: Ptr) -> Ptr {
    let view = unsafe { ID3D11View::from_raw_borrowed(&raw) }.unwrap();
    owned(unsafe { view.GetResource() }.ok())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_texture_device(raw: Ptr) -> Ptr {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_describe_texture(raw: Ptr, out: &mut Texture) {
    let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.unwrap();
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    *out = unsafe { std::mem::transmute(desc) };
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_describe_view(raw: Ptr, kind: u32, out: &mut View) {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_format_support(raw: Ptr, format: u32) -> u32 {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    unsafe { device.CheckFormatSupport(DXGI_FORMAT(format as i32)) }.unwrap_or(0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_texture(
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_view(
    raw: Ptr,
    texture: Ptr,
    kind: u32,
    source: Option<&View>,
) -> Ptr {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_constant_buffer(raw: Ptr) -> Ptr {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_raster_state(raw: Ptr) -> Ptr {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_blend_state(raw: Ptr) -> Ptr {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_depth_state(raw: Ptr, write_depth: u32) -> Ptr {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_update_constants(raw: Ptr, buffer: Ptr, data: *const u32) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let buffer = unsafe { ID3D11Buffer::from_raw_borrowed(&buffer) }.unwrap();
    unsafe { context.UpdateSubresource(buffer, 0, None, data.cast(), 0, 0) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_ps_constant(raw: Ptr, slot: u32, buffer: Ptr) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    // The interface option has the same ABI as this nullable COM pointer.
    unsafe {
        context.PSSetConstantBuffers(
            slot,
            Some(slice::from_raw_parts(std::ptr::from_ref(&buffer).cast(), 1)),
        )
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_targets(raw: Ptr, count: u32, targets: *const Ptr, depth: Ptr) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let depth = unsafe { ID3D11DepthStencilView::from_raw_borrowed(&depth) };
    // The pass always supplies a non-null pointer to its nullable RTV array.
    let targets = unsafe { slice::from_raw_parts(targets.cast(), count as usize) };
    unsafe {
        context.OMSetRenderTargetsAndUnorderedAccessViews(Some(targets), depth, 0, 0, None, None)
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_ps_resources(
    raw: Ptr,
    slot: u32,
    count: u32,
    inputs: *const Ptr,
) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let inputs = unsafe { slice::from_raw_parts(inputs.cast(), count as usize) };
    unsafe { context.PSSetShaderResources(slot, Some(inputs)) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_clear_stencil(raw: Ptr, depth: Ptr) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let depth = unsafe { ID3D11DepthStencilView::from_raw_borrowed(&depth) }.unwrap();
    unsafe { context.ClearDepthStencilView(depth, D3D11_CLEAR_STENCIL.0 as u32, 1.0, 0) };
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

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_anomaly_sr_set_full_viewport(
    raw: Ptr,
    width: u32,
    height: u32,
) {
    if let Some(context) = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }
        && width != 0
        && height != 0
    {
        viewport(context, Extent { width, height });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_draw(raw: Ptr, draw: &Draw) {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_timer_retain_device(raw: Ptr) -> Ptr {
    unsafe { rsf_anomaly_sr_retain(raw) };
    raw
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_timer_create_query(raw: Ptr, disjoint: bool) -> Ptr {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_timer_issue(raw: Ptr, query: Ptr, begin: bool) {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_timer_clock(
    raw: Ptr,
    query: Ptr,
    frequency: &mut u64,
    disjoint: &mut bool,
) -> i32 {
    let mut data = D3D11_QUERY_DATA_TIMESTAMP_DISJOINT::default();
    let result = unsafe { query_data(raw, query, &mut data) };
    if result == 0 {
        *frequency = data.Frequency;
        *disjoint = data.Disjoint.as_bool();
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_timer_timestamp(raw: Ptr, query: Ptr, ticks: *mut u64) -> i32 {
    unsafe { query_data(raw, query, ticks) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_immediate_context(raw: Ptr) -> bool {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    unsafe { context.GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_context_device(raw: Ptr) -> Ptr {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    owned(unsafe { context.GetDevice() }.ok())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_copy_subresource(
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_map_read(
    raw: Ptr,
    texture: Ptr,
    subresource: u32,
    data: &mut *const c_void,
    row_pitch: &mut u32,
) -> bool {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let texture = unsafe { ID3D11Resource::from_raw_borrowed(&texture) }.unwrap();
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    if unsafe { context.Map(texture, subresource, D3D11_MAP_READ, 0, Some(&mut mapped)) }.is_err() {
        return false;
    }
    *data = mapped.pData.cast_const();
    *row_pitch = mapped.RowPitch;
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_unmap(raw: Ptr, texture: Ptr, subresource: u32) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let texture = unsafe { ID3D11Resource::from_raw_borrowed(&texture) }.unwrap();
    unsafe { context.Unmap(texture, subresource) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_twin_refs(raw: Ptr) -> u32 {
    let object = unsafe { IUnknown::from_raw_borrowed(&raw) }.unwrap();
    unsafe {
        (object.vtable().AddRef)(raw);
        (object.vtable().Release)(raw) - 1
    }
}
