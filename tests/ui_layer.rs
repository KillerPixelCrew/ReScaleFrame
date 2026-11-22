// SPDX-License-Identifier: MIT
use super::{Device, Handle, Pass, TestResult, check, near};
use crate::graphics_d3d11::{constant_buffer, pixel_shader, shader_code, vertex_shader};
use crate::ui_layer_abi::{
    RSF_UI_LAYER_ABI_VERSION, rsf_ui_layer_setup as Setup, rsf_ui_layer_status as Status,
};
use rsf_shader_compiler::Compiler;
use std::ptr::{self, null_mut};
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
use windows::core::Interface;

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const WIDTH: u32 = 8;
const HEIGHT: u32 = 8;

unsafe extern "C" {
    fn rsf_ui_layer_create(device: Handle, setup: *const Setup, layer: *mut Handle) -> i32;
    fn rsf_ui_layer_destroy(layer: Handle);
    fn rsf_ui_layer_begin_frame(layer: Handle, context: Handle) -> i32;
    fn rsf_ui_layer_target(layer: Handle) -> Handle;
    fn rsf_ui_layer_source(layer: Handle) -> Handle;
    fn rsf_ui_layer_texture(layer: Handle) -> Handle;
    fn rsf_ui_layer_mark_written(layer: Handle);
    fn rsf_ui_layer_get_status(layer: Handle, status: *mut Status) -> i32;
}

fn status(layer: &Pass) -> TestResult<Status> {
    let mut result = Status {
        struct_size: size_of::<Status>() as u32,
        width: 0,
        height: 0,
        slot: 0,
        shareable: 0,
        written_this_frame: 0,
        frames_begun: 0,
        frames_written: 0,
        clears: 0,
    };
    check(
        unsafe { rsf_ui_layer_get_status(layer.raw, &mut result) } == OK,
        "UI layer status",
    )?;
    Ok(result)
}

fn begin(layer: &Pass, gpu: &Device) -> TestResult {
    check(
        unsafe { rsf_ui_layer_begin_frame(layer.raw, gpu.context.as_raw()) } == OK,
        "UI layer frame begins",
    )
}

fn read_pixel(layer: &Pass, gpu: &Device) -> TestResult<[f32; 4]> {
    let texture = unsafe { rsf_ui_layer_texture(layer.raw) };
    let texture =
        unsafe { ID3D11Texture2D::from_raw_borrowed(&texture) }.ok_or("layer texture missing")?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    check(
        desc.Format == DXGI_FORMAT_R8G8B8A8_UNORM,
        "UI texture is RGBA8 UNORM",
    )?;
    let pixels = unsafe { gpu.read::<u8>(texture, &desc, 4) }?;
    let rgba: [u8; 4] = pixels
        .get(..4)
        .ok_or("first UI pixel missing")?
        .try_into()?;
    Ok(rgba.map(|channel| f32::from(channel) / 255.0))
}

const SHADER: &[u8] = br#"
cbuffer Params : register(b0) { float4 Colour; };
float4 vertex_main(uint id : SV_VertexID) : SV_Position
{
    float2 uv = float2((id << 1) & 2, id & 2);
    return float4(uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
}
float4 pixel_main() : SV_Target { return Colour; }
"#;

fn blend_cases(layer: &Pass, gpu: &Device) -> TestResult {
    if Compiler::system("d3dcompiler_47.dll")
        .and_then(|compiler| compiler.entry())
        .is_none()
    {
        eprintln!("d3dcompiler_47.dll unavailable, skipping UI blend cases");
        return Ok(());
    }
    let vs = shader_code(SHADER, c"ui_layer_test", c"vertex_main", c"vs_5_0", 0)?;
    let ps = shader_code(SHADER, c"ui_layer_test", c"pixel_main", c"ps_5_0", 0)?;
    let vertex = vertex_shader(&gpu.device, &vs).ok_or("UI vertex shader creation")?;
    let pixel = pixel_shader(&gpu.device, &ps).ok_or("UI pixel shader creation")?;
    let color = constant_buffer(&gpu.device, 16, true).ok_or("UI color buffer creation")?;

    // The base pass never adds to alpha. The diverted version accumulates coverage, unless
    // the original render target's write mask still excludes alpha.
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
        BlendEnable: true.into(),
        SrcBlend: D3D11_BLEND_SRC_ALPHA,
        DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
        BlendOp: D3D11_BLEND_OP_ADD,
        SrcBlendAlpha: D3D11_BLEND_ZERO,
        DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
        BlendOpAlpha: D3D11_BLEND_OP_ADD,
        RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };
    let blend_state = |desc: &D3D11_BLEND_DESC| -> TestResult<ID3D11BlendState> {
        let mut state = None;
        unsafe { gpu.device.CreateBlendState(desc, Some(&mut state)) }?;
        state.ok_or_else(|| "UI blend state creation returned null".into())
    };
    let unpatched = blend_state(&desc)?;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    let patched = blend_state(&desc)?;
    desc.RenderTarget[0].RenderTargetWriteMask = (D3D11_COLOR_WRITE_ENABLE_RED.0
        | D3D11_COLOR_WRITE_ENABLE_GREEN.0
        | D3D11_COLOR_WRITE_ENABLE_BLUE.0) as u8;
    let masked = blend_state(&desc)?;

    let context = &gpu.context;
    let draw_once = |blend: &ID3D11BlendState| -> TestResult {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { context.Map(&color, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped)) }?;
        if mapped.pData.is_null() {
            unsafe { context.Unmap(&color, 0) };
            return Err("UI color mapping returned null".into());
        }
        unsafe {
            mapped.pData.cast::<[f32; 4]>().write([1.0, 1.0, 1.0, 0.5]);
            context.Unmap(&color, 0);
        }
        let target = unsafe { rsf_ui_layer_target(layer.raw) };
        let target = unsafe { ID3D11RenderTargetView::from_raw_borrowed(&target) }
            .ok_or("layer RTV missing")?;
        unsafe {
            context.OMSetRenderTargets(
                Some(&[Some(target.clone())]),
                None::<&ID3D11DepthStencilView>,
            );
            context.VSSetShader(&vertex, None);
            context.PSSetShader(&pixel, None);
            context.PSSetConstantBuffers(0, Some(&[Some(color.clone())]));
            context.IASetInputLayout(None::<&ID3D11InputLayout>);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.OMSetBlendState(blend, Some(&[0.0; 4]), u32::MAX);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                Width: WIDTH as f32,
                Height: HEIGHT as f32,
                MaxDepth: 1.0,
                ..Default::default()
            }]));
            context.Draw(3, 0);
        }
        Ok(())
    };

    begin(layer, gpu)?;
    draw_once(&unpatched)?;
    let [red, _, _, alpha] = read_pixel(layer, gpu)?;
    near(alpha, 0.0, 1.0 / 255.0, "unpatched alpha stays clear")?;
    near(red, 0.5, 2.0 / 255.0, "unpatched color still lands")?;

    begin(layer, gpu)?;
    draw_once(&patched)?;
    near(
        read_pixel(layer, gpu)?[3],
        0.5,
        2.0 / 255.0,
        "patched alpha accumulates",
    )?;
    draw_once(&patched)?;
    near(
        read_pixel(layer, gpu)?[3],
        0.75,
        3.0 / 255.0,
        "second draw uses alpha-over",
    )?;

    begin(layer, gpu)?;
    draw_once(&masked)?;
    let [red, _, _, alpha] = read_pixel(layer, gpu)?;
    near(
        alpha,
        0.0,
        1.0 / 255.0,
        "write mask still prevents alpha accumulation",
    )?;
    near(
        red,
        0.5,
        2.0 / 255.0,
        "masked alpha leaves color writes enabled",
    )
}

fn run(gpu: &Device) -> TestResult {
    let mut setup = Setup {
        struct_size: size_of::<Setup>() as u32,
        abi_version: RSF_UI_LAYER_ABI_VERSION + 1,
        width: WIDTH,
        height: HEIGHT,
        shareable: 0,
        srgb: 0,
        log: None,
        log_user: null_mut(),
    };
    let mut layer = null_mut();
    let device = gpu.device.as_raw();
    check(
        unsafe { rsf_ui_layer_create(device, &setup, &mut layer) } == ABI_MISMATCH,
        "wrong layer ABI refused",
    )?;
    setup.abi_version = RSF_UI_LAYER_ABI_VERSION;
    setup.width = 0;
    check(
        unsafe { rsf_ui_layer_create(device, &setup, &mut layer) } == INVALID,
        "zero layer extent refused",
    )?;
    setup.width = WIDTH;
    check(
        unsafe { rsf_ui_layer_create(device, &setup, &mut layer) } == OK && !layer.is_null(),
        "layer created",
    )?;
    let layer = Pass {
        raw: layer,
        destroy: rsf_ui_layer_destroy,
    };
    check(
        unsafe {
            rsf_ui_layer_target(layer.raw).is_null() && rsf_ui_layer_source(layer.raw).is_null()
        },
        "no current target or source before begin",
    )?;

    begin(&layer, gpu)?;
    let [red, _, _, alpha] = read_pixel(&layer, gpu)?;
    check(
        red == 0.0 && alpha == 0.0,
        "frame starts cleared to transparent zero",
    )?;
    let first = status(&layer)?.slot;
    let first_texture = unsafe { rsf_ui_layer_texture(layer.raw) };
    begin(&layer, gpu)?;
    check(
        status(&layer)?.slot != first,
        "second frame uses the other slot",
    )?;
    check(
        unsafe { rsf_ui_layer_texture(layer.raw) } != first_texture,
        "second slot has a different texture",
    )?;
    begin(&layer, gpu)?;
    check(status(&layer)?.slot == first, "third frame cycles back")?;

    begin(&layer, gpu)?;
    let before = status(&layer)?;
    check(before.written_this_frame == 0, "fresh frame is unwritten")?;
    unsafe {
        rsf_ui_layer_mark_written(layer.raw);
        rsf_ui_layer_mark_written(layer.raw);
    }
    let written = status(&layer)?;
    check(
        written.written_this_frame == 1 && written.frames_written == before.frames_written + 1,
        "multiple draws mark exactly one frame written",
    )?;
    begin(&layer, gpu)?;
    check(
        status(&layer)?.written_this_frame == 0,
        "next frame starts unwritten",
    )?;
    blend_cases(&layer, gpu)?;

    check(
        unsafe { rsf_ui_layer_begin_frame(null_mut(), gpu.context.as_raw()) } == INVALID,
        "null layer refused",
    )?;
    check(
        unsafe { rsf_ui_layer_begin_frame(layer.raw, null_mut()) } == INVALID,
        "null context refused",
    )?;
    let mut short = status(&layer)?;
    short.struct_size = 4;
    check(
        unsafe { rsf_ui_layer_get_status(layer.raw, ptr::from_mut(&mut short)) } == INVALID,
        "short status refused",
    )?;
    check(
        unsafe { rsf_ui_layer_target(null_mut()) }.is_null(),
        "null layer has no target",
    )?;
    unsafe {
        rsf_ui_layer_mark_written(null_mut());
        rsf_ui_layer_destroy(null_mut());
        gpu.context.ClearState();
    }
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ui_layer_fixture() -> i32 {
    let Some(gpu) = Device::feature_11() else {
        eprintln!("No D3D11 feature-level 11 device available, skipping UI layer fixture");
        return 77;
    };
    match run(&gpu) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("UI layer: {error}");
            1
        }
    }
}
