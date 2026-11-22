// SPDX-License-Identifier: MIT
use super::{Device, Handle, TestResult, check};
use crate::sys::rsf_d3d11_state as State;
use std::ptr::null_mut;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_PRIMITIVE_TOPOLOGY_POINTLIST,
    D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::core::Interface;

type Save = unsafe extern "C" fn(Handle, *mut State) -> u32;
type Restore = unsafe extern "C" fn(Handle, *mut State);

unsafe extern "C" {
    fn rsf_d3d11_state_save(context: Handle, state: *mut State) -> u32;
    fn rsf_d3d11_state_save_independent(context: Handle, state: *mut State) -> u32;
    fn rsf_d3d11_state_restore(context: Handle, state: *mut State);
}

struct Scope {
    context: Handle,
    state: State,
    restore: Restore,
}

impl Scope {
    fn new(context: &ID3D11DeviceContext, save: Save, restore: Restore) -> TestResult<Self> {
        let context = context.as_raw();
        let mut state = State::default();
        check(
            unsafe { save(context, &mut state) } == 1,
            "state save succeeds",
        )?;
        Ok(Self {
            context,
            state,
            restore,
        })
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        unsafe { (self.restore)(self.context, &mut self.state) };
    }
}

fn slot<T>(get: impl FnOnce(&mut [Option<T>])) -> Option<T> {
    let mut result = None;
    get(std::slice::from_mut(&mut result));
    result
}

fn viewport(width: f32, height: f32, max_depth: f32) -> D3D11_VIEWPORT {
    D3D11_VIEWPORT {
        Width: width,
        Height: height,
        MaxDepth: max_depth,
        ..Default::default()
    }
}

fn scissor(right: i32, bottom: i32) -> RECT {
    RECT {
        left: 0,
        top: 0,
        right,
        bottom,
    }
}

fn run(gpu: &Device) -> TestResult {
    let device = &gpu.device;
    let context = &gpu.context;
    let mut state = State::default();
    check(
        unsafe { rsf_d3d11_state_save(null_mut(), &mut state) } == 0,
        "null context is refused",
    )?;
    check(
        unsafe { rsf_d3d11_state_save(context.as_raw(), null_mut()) } == 0,
        "null storage is refused",
    )?;

    // Separate read and write allocations so D3D11 does not implicitly unbind them.
    let texture = |side| {
        gpu.texture(
            &Device::desc(
                [side; 2],
                DXGI_FORMAT_B8G8R8A8_UNORM,
                D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE,
            ),
            None,
        )
    };
    let (first, second, drawn_into) = (texture(64)?, texture(32)?, texture(64)?);
    let mut rtv = None;
    let mut first_view = None;
    let mut second_view = None;
    let mut sampler = None;
    let mut constants = None;
    let mut raster = None;
    unsafe {
        device.CreateRenderTargetView(&drawn_into, None, Some(&mut rtv))?;
        device.CreateShaderResourceView(&first, None, Some(&mut first_view))?;
        device.CreateShaderResourceView(&second, None, Some(&mut second_view))?;
        device.CreateSamplerState(
            &D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_POINT,
                AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                ..Default::default()
            },
            Some(&mut sampler),
        )?;
        device.CreateBuffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: 256,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                ..Default::default()
            },
            None,
            Some(&mut constants),
        )?;
        device.CreateRasterizerState(
            &D3D11_RASTERIZER_DESC {
                FillMode: D3D11_FILL_SOLID,
                CullMode: D3D11_CULL_NONE,
                ..Default::default()
            },
            Some(&mut raster),
        )?;
    }
    let rtv = rtv.ok_or("missing RTV")?;
    let first_view = first_view.ok_or("missing first SRV")?;
    let second_view = second_view.ok_or("missing second SRV")?;
    let sampler = sampler.ok_or("missing sampler")?;
    let constants = constants.ok_or("missing constants")?;
    let raster = raster.ok_or("missing rasterizer")?;
    let get_pixel = |index| slot(|out| unsafe { context.PSGetShaderResources(index, Some(out)) });
    let get_target = || slot(|out| unsafe { context.OMGetRenderTargets(Some(out), None) });
    let get_sampler = || slot(|out| unsafe { context.PSGetSamplers(0, Some(out)) });
    let clear_pixel = |index| unsafe { context.PSSetShaderResources(index, Some(&[None])) };
    let set_raster = || unsafe {
        context.RSSetState(&raster);
        context.RSSetViewports(Some(&[viewport(64.0, 48.0, 1.0)]));
        context.RSSetScissorRects(Some(&[scissor(64, 48)]));
    };
    let clear_raster = || unsafe {
        context.RSSetState(None::<&ID3D11RasterizerState>);
        context.RSSetViewports(Some(&[viewport(8.0, 8.0, 0.0)]));
        context.RSSetScissorRects(Some(&[scissor(8, 8)]));
    };
    let verify_raster = || -> TestResult {
        check(
            unsafe { context.RSGetState() }.ok().as_ref() == Some(&raster),
            "rasterizer restored",
        )?;
        let mut viewport = D3D11_VIEWPORT::default();
        let mut count = 1;
        unsafe { context.RSGetViewports(&mut count, Some(&mut viewport)) };
        check(
            count == 1 && viewport.Width == 64.0 && viewport.Height == 48.0,
            "viewport restored",
        )?;
        let mut rect = RECT::default();
        count = 1;
        unsafe { context.RSGetScissorRects(&mut count, Some(&mut rect)) };
        check(
            count == 1 && rect.right == 64 && rect.bottom == 48,
            "scissor restored",
        )
    };
    unsafe {
        context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None::<&ID3D11DepthStencilView>);
        context.PSSetShaderResources(3, Some(&[Some(first_view.clone())]));
        context.CSSetShaderResources(1, Some(&[Some(second_view.clone())]));
        context.PSSetSamplers(0, Some(&[Some(sampler.clone())]));
        context.PSSetConstantBuffers(2, Some(&[Some(constants.clone())]));
        context.VSSetConstantBuffers(0, Some(&[Some(constants.clone())]));
        context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
    }
    set_raster();

    let save = || Scope::new(context, rsf_d3d11_state_save, rsf_d3d11_state_restore);
    let full = save()?;
    unsafe {
        context.OMSetRenderTargets(None, None::<&ID3D11DepthStencilView>);
        context.CSSetShaderResources(1, Some(&[None]));
        context.PSSetSamplers(0, Some(&[None]));
        context.PSSetConstantBuffers(2, Some(&[None]));
        context.VSSetConstantBuffers(0, Some(&[None]));
        context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_POINTLIST);
    }
    clear_pixel(3);
    clear_raster();
    check(get_pixel(3).is_none(), "state was disturbed before restore")?;
    drop(full);
    check(
        get_target().as_ref() == Some(&rtv),
        "render target restored",
    )?;
    check(
        get_pixel(3).as_ref() == Some(&first_view),
        "pixel SRV restored to slot 3",
    )?;
    check(
        slot(|out| unsafe { context.CSGetShaderResources(1, Some(out)) }).as_ref()
            == Some(&second_view),
        "compute SRV restored to slot 1",
    )?;
    check(
        get_sampler().as_ref() == Some(&sampler),
        "pixel sampler restored",
    )?;
    check(
        slot(|out| unsafe { context.PSGetConstantBuffers(2, Some(out)) }).as_ref()
            == Some(&constants),
        "pixel constants restored to slot 2",
    )?;
    check(
        slot(|out| unsafe { context.VSGetConstantBuffers(0, Some(out)) }).as_ref()
            == Some(&constants),
        "vertex constants restored to slot 0",
    )?;
    check(
        unsafe { context.IAGetPrimitiveTopology() } == D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
        "full scope restored strip topology",
    )?;
    verify_raster()?;

    let outer = save()?;
    clear_pixel(3);
    let independent = Scope::new(
        context,
        rsf_d3d11_state_save_independent,
        rsf_d3d11_state_restore,
    )?;
    unsafe { context.PSSetShaderResources(3, Some(&[Some(first_view.clone())])) };
    drop(independent);
    check(
        get_pixel(3).is_none(),
        "independent scope restored the binding inside the outer scope",
    )?;
    drop(save()?);
    check(
        get_pixel(3).is_none(),
        "nested restore leaves the cleared binding alone",
    )?;
    drop(outer);
    check(
        get_pixel(3).as_ref() == Some(&first_view),
        "outer restore restored the original binding",
    )?;
    let fresh = save()?;
    clear_pixel(3);
    drop(fresh);
    check(
        get_pixel(3).as_ref() == Some(&first_view),
        "a fresh scope saves after the outer one closes",
    )?;
    unsafe { context.ClearState() };
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_d3d11_state_fixture() -> i32 {
    let Some(gpu) = Device::create(D3D_DRIVER_TYPE_HARDWARE, None) else {
        eprintln!("No hardware D3D11 device available, skipping state fixture");
        return 0;
    };
    match run(&gpu) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("D3D11 state: {error}");
            1
        }
    }
}
