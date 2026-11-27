// SPDX-License-Identifier: MIT
use super::{Facts, Params, plan};
use rsf_shader_compiler::{Request, compile_system};
use std::ffi::{CStr, c_void};
use std::ptr::from_ref;
use windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE2D;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCOMPILE_ENABLE_STRICTNESS;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS,
    D3D11_BUFFER_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    ID3D11Buffer, ID3D11ComputeShader, ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView,
    ID3D11Texture2D, ID3D11UnorderedAccessView,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R32_FLOAT, DXGI_SAMPLE_DESC,
};
use windows::core::Interface;

fn compile(device: &ID3D11Device, source: &[u8], name: &CStr) -> Option<ID3D11ComputeShader> {
    let code = compile_system(Request {
        source,
        name,
        entry: c"main",
        target: c"cs_5_0",
        flags: D3DCOMPILE_ENABLE_STRICTNESS,
    })?;
    let mut shader = None;
    unsafe { device.CreateComputeShader(&code, None, Some(&mut shader)) }.ok()?;
    shader
}
/// TrueSky's one-to-one depth-bounds kernel. The native effect owns dispatch and bindings.
pub(crate) fn depth_shader(device: &ID3D11Device) -> Option<ID3D11ComputeShader> {
    compile(
        device,
        include_bytes!("../truesky_depth_bounds.hlsl"),
        c"AC7TrueSkyDepth1",
    )
}

/// composite_tile's cloud-depth kernel and target, made on its device at its extent.
#[derive(Default)]
pub(crate) struct CloudDepth {
    device: Option<ID3D11Device>,
    pipeline: Option<(ID3D11ComputeShader, ID3D11Buffer)>,
    target: Option<(ID3D11Texture2D, ID3D11UnorderedAccessView)>,
    width: u32,
    height: u32,
    /// The kernel failed to build on this device.
    refused: bool,
}

fn texture_desc(view: &ID3D11ShaderResourceView) -> Option<D3D11_TEXTURE2D_DESC> {
    let mut srv = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
    unsafe { view.GetDesc(&mut srv) };
    if srv.ViewDimension != D3D_SRV_DIMENSION_TEXTURE2D
        || srv.Format != DXGI_FORMAT_R16G16B16A16_FLOAT
    {
        return None;
    }
    let texture = unsafe { view.GetResource() }
        .ok()?
        .cast::<ID3D11Texture2D>()
        .ok()?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    (desc.SampleDesc.Count == 1 && desc.ArraySize == 1).then_some(desc)
}

fn pipeline(device: &ID3D11Device) -> Option<(ID3D11ComputeShader, ID3D11Buffer)> {
    let shader = compile(
        device,
        include_bytes!("../truesky_cloud_motion.hlsl"),
        c"AC7TrueSkyCloudDepth",
    )?;
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: size_of::<Params>() as u32,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        ..Default::default()
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) }.ok()?;
    Some((shader, buffer?))
}

fn target(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Option<(ID3D11Texture2D, ID3D11UnorderedAccessView)> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R32_FLOAT,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BindFlags: (D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS).0 as u32,
        ..Default::default()
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.ok()?;
    let texture = texture?;
    let mut view = None;
    unsafe { device.CreateUnorderedAccessView(&texture, None, Some(&mut view)) }.ok()?;
    Some((texture, view?))
}

fn prepare(state: &mut CloudDepth, device: &ID3D11Device, [width, height]: [u32; 2]) -> Option<()> {
    if state.device.as_ref().map(Interface::as_raw) != Some(device.as_raw()) {
        *state = CloudDepth {
            device: Some(device.clone()),
            ..CloudDepth::default()
        };
    }
    if state.refused {
        return None;
    }
    if state.pipeline.is_none() {
        state.refused = true;
        state.pipeline = Some(pipeline(device)?);
        state.refused = false;
    }
    if state.target.is_none() || state.width != width || state.height != height {
        state.target = Some(target(device, width, height)?);
        state.width = width;
        state.height = height;
    }
    Some(())
}

/// Writes composite_tile's cloud depth for the bound draw and returns the borrowed target
/// texture, or None when the bindings differ from the measured layout. `width` and `height` are
/// the draw's viewport.
pub(crate) fn write(
    state: &mut CloudDepth,
    context: &ID3D11DeviceContext,
    width: f32,
    height: f32,
) -> Option<*mut c_void> {
    let mut inputs = [None, None];
    let mut hdr = [None];
    unsafe {
        context.PSGetShaderResources(1, Some(&mut inputs));
        context.PSGetConstantBuffers(12, Some(&mut hdr));
    }
    let [hdr] = hdr;
    let hdr = hdr?;
    let far = texture_desc(inputs[0].as_ref()?)?;
    let grid = texture_desc(inputs[1].as_ref()?)?;
    let mut hdr_desc = D3D11_BUFFER_DESC::default();
    unsafe { hdr.GetDesc(&mut hdr_desc) };
    let params = plan(&Facts {
        viewport_width: width,
        viewport_height: height,
        far_width: far.Width,
        far_height: far.Height,
        grid_width: grid.Width,
        grid_height: grid.Height,
        hdr_bytes: hdr_desc.ByteWidth,
    })?;
    let device = unsafe { context.GetDevice() }.ok()?;
    prepare(state, &device, params.size)?;
    let (shader, constants) = state.pipeline.as_ref()?;
    let (texture, view) = state.target.as_ref()?;
    let buffers = [Some(hdr), Some(constants.clone())];
    let output = Some(view.clone());

    let mut saved_shader = None;
    let mut saved_instances = [const { None }; 256];
    let mut instance_count = saved_instances.len() as u32;
    let mut saved_inputs = [None, None];
    let mut saved_view = [None];
    let mut saved_buffers = [None, None];
    unsafe {
        context.UpdateSubresource(constants, 0, None, from_ref(&params).cast(), 0, 0);
        context.CSGetShader(
            &mut saved_shader,
            Some(saved_instances.as_mut_ptr()),
            Some(&mut instance_count),
        );
        context.CSGetShaderResources(0, Some(&mut saved_inputs));
        context.CSGetUnorderedAccessViews(0, Some(&mut saved_view));
        context.CSGetConstantBuffers(0, Some(&mut saved_buffers));
        context.CSSetShader(shader, None);
        context.CSSetShaderResources(0, Some(&inputs));
        context.CSSetUnorderedAccessViews(0, 1, Some(&output), None);
        context.CSSetConstantBuffers(0, Some(&buffers));
        context.Dispatch(params.size[0].div_ceil(8), params.size[1].div_ceil(8), 1);
        let count = (instance_count as usize).min(saved_instances.len());
        context.CSSetShader(saved_shader.as_ref(), Some(&saved_instances[..count]));
        context.CSSetShaderResources(0, Some(&saved_inputs));
        context.CSSetUnorderedAccessViews(0, 1, Some(saved_view.as_ptr()), None);
        context.CSSetConstantBuffers(0, Some(&saved_buffers));
    }
    Some(texture.as_raw())
}
