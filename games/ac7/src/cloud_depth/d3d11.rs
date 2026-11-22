// SPDX-License-Identifier: MIT
use super::{Facts, Params, plan};
use crate::com_resource::{release, retain};
use crate::sys::rsf_ac7_cloud_depth as State;
use rsf_shader_compiler::{Request, compile_system};
use std::ffi::{CStr, c_void};
use std::ptr::{from_ref, null_mut};
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
/// The native effect owns dispatch and bindings. This returns one shader reference.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_ac7_create_truesky_depth_shader(
    device: *mut c_void,
) -> *mut c_void {
    let Some(device) = (unsafe { ID3D11Device::from_raw_borrowed(&device) }) else {
        return null_mut();
    };
    compile(
        device,
        include_bytes!("../truesky_depth_bounds.hlsl"),
        c"AC7TrueSkyDepth1",
    )
    .map_or(null_mut(), Interface::into_raw)
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

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_ac7_cloud_depth_release(state: &mut State) {
    for resource in [
        state.view,
        state.texture,
        state.constants,
        state.shader,
        state.device,
    ] {
        unsafe { release(resource) };
    }
    *state = State::default();
}

fn prepare(state: &mut State, device: &ID3D11Device, [width, height]: [u32; 2]) -> Option<()> {
    if state.device != device.as_raw() {
        rsf_ac7_cloud_depth_release(state);
        state.device = device.as_raw();
        unsafe { retain(state.device) };
    }
    if state.refused {
        return None;
    }
    if state.shader.is_null() {
        state.refused = true;
        let (shader, constants) = pipeline(device)?;
        state.shader = shader.into_raw();
        state.constants = constants.into_raw();
        state.refused = false;
    }
    if state.texture.is_null() || state.width != width || state.height != height {
        let (texture, view) = target(device, width, height)?;
        unsafe {
            release(state.view);
            release(state.texture);
        }
        state.texture = texture.into_raw();
        state.view = view.into_raw();
        state.width = width;
        state.height = height;
    }
    Some(())
}

fn write(
    state: &mut State,
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
    let constants = unsafe { ID3D11Buffer::from_raw_borrowed(&state.constants) }?;
    let shader = unsafe { ID3D11ComputeShader::from_raw_borrowed(&state.shader) }?;
    let view = unsafe { ID3D11UnorderedAccessView::from_raw_borrowed(&state.view) }?;
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
    Some(state.texture)
}

/// The adapter lends the immediate context and exclusively lends initialized state.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_ac7_cloud_depth_write(
    state: &mut State,
    context: *mut c_void,
    width: f32,
    height: f32,
) -> *mut c_void {
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }) else {
        return null_mut();
    };
    write(state, context, width, height).unwrap_or(null_mut())
}
