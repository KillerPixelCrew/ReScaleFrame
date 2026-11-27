// SPDX-License-Identifier: MIT
//! Executes the locally compiled stock and patched FXC bytecode in D3D11.

use rsf_shader_compiler::{Request, compile_system_checked};
use rsf_test_support::device::Device;
use std::{env, ffi::CStr, fs, path::Path, ptr, slice};
use windows::Win32::Graphics::Direct3D::{Fxc::*, *};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::{Interface, PCSTR};

type Fixture<T> = Result<T, String>;
type V4 = [f32; 4];
type Matrix = [V4; 4];
type Pixels = [Vec<V4>; 3];
const HISTORY: V4 = [1.0; 4];
const NO_LINKAGE: Option<&ID3D11ClassLinkage> = None;
const NO_DEPTH: Option<&ID3D11DepthStencilView> = None;
const WIDTH: u32 = 16;
const PIXELS: u32 = WIDTH * WIDTH;
const SENTINEL: f32 = -999.0;

fn check(condition: bool, message: &str) -> Fixture<()> {
    condition.then_some(()).ok_or_else(|| message.into())
}

fn win<T>(result: windows::core::Result<T>, operation: &str) -> Fixture<T> {
    result.map_err(|error| format!("{operation}: {error}"))
}

fn created<T>(
    operation: &str,
    create: impl FnOnce(*mut Option<T>) -> windows::core::Result<()>,
) -> Fixture<T> {
    let mut out = None;
    win(create(&mut out), operation)?;
    out.ok_or_else(|| format!("{operation} returned no object"))
}

fn name(value: PCSTR) -> Fixture<String> {
    check(!value.is_null(), "missing reflected name")?;
    // Reflection owns the NUL-terminated string through the descriptor read.
    unsafe { CStr::from_ptr(value.0.cast()) }
        .to_str()
        .map(str::to_owned)
        .map_err(|error| error.to_string())
}

fn close(actual: f32, expected: f32, tolerance: f32, label: &str) -> Fixture<()> {
    if actual.is_finite() && expected.is_finite() && (actual - expected).abs() <= tolerance {
        return Ok(());
    }
    Err(format!("{label}: actual={actual} expected={expected}"))
}

fn exact(actual: f32, expected: f32, label: &str) -> Fixture<()> {
    close(actual, expected, 0.00002, label)
}

fn matrix(previous: bool, hud: bool) -> Matrix {
    if previous {
        [
            [0.8, 0.07, 0.0, if hud { -0.4 } else { 0.12 }],
            [0.03, 1.1, 0.04, -0.08],
            [0.0, 0.05, 0.9, 0.02],
            [0.04, 0.02, 0.01, if hud { 0.75 } else { 1.15 }],
        ]
    } else {
        [
            [1.2, 0.02, 0.03, 0.1],
            [0.01, 0.9, 0.02, -0.05],
            [0.02, 0.03, 1.1, 0.04],
            [0.1, 0.03, 0.02, 1.1],
        ]
    }
}

fn mul(matrix: Matrix, vector: V4) -> V4 {
    matrix.map(|row| row.into_iter().zip(vector).map(|(m, v)| m * v).sum())
}

fn palette(previous: bool) -> [V4; 234] {
    let mut rows = [[0.0; 4]; 234];
    let offset = if previous { 0.13 } else { 0.02 };
    for (bone, row) in rows.chunks_exact_mut(3).enumerate() {
        let n = bone as f32;
        row[0] = [0.9 + 0.01 * n, 0.02, 0.01, offset + 0.01 * n];
        row[1] = [0.01, 1.05 + 0.01 * n, 0.03, -offset - 0.02 * n];
        row[2] = [0.03, 0.01, 0.95, offset + 0.03 * n];
    }
    rows
}

fn deformed(mode: &str, previous: bool) -> Fixture<V4> {
    let point = [0.1, -0.2, 0.3, 1.0];
    let (indices, weights) = match mode {
        "STATIC" | "NONE" => return Ok([0.1, -0.2, 0.3, 0.7]),
        "0" => return Ok(point),
        "1" => ([3, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
        "2" => ([3, 6, 0, 0], [0.8, 0.2, 0.0, 0.0]),
        "3" => ([3, 6, 9, 0], [0.2, 0.3, 0.5, 0.0]),
        // Packed D3DCOLOR index xyz swizzle.
        "4" => ([6, 9, 12, 3], [0.2, 0.3, 0.1, 0.4]),
        _ => return Err(format!("unknown skin mode {mode}")),
    };
    let bones = palette(previous);
    let mut result = [0.0, 0.0, 0.0, 1.0];
    for (row, value) in result[..3].iter_mut().enumerate() {
        for (index, weight) in indices.into_iter().zip(weights) {
            for (bone, point) in bones[index + row].into_iter().zip(point) {
                *value += bone * weight * point;
            }
        }
    }
    Ok(result)
}

fn reflect(code: &[u8]) -> Fixture<ID3D11ShaderReflection> {
    let mut raw = ptr::null_mut();
    unsafe {
        win(
            D3DReflect(
                code.as_ptr().cast(),
                code.len(),
                &ID3D11ShaderReflection::IID,
                &mut raw,
            ),
            "D3DReflect",
        )?;
        check(!raw.is_null(), "D3DReflect returned no shader")?;
        Ok(ID3D11ShaderReflection::from_raw(raw))
    }
}

fn description(shader: &ID3D11ShaderReflection) -> Fixture<D3D11_SHADER_DESC> {
    let mut desc = D3D11_SHADER_DESC::default();
    win(unsafe { shader.GetDesc(&mut desc) }, "shader reflection")?;
    Ok(desc)
}

fn binding(shader: &ID3D11ShaderReflection, index: u32) -> Fixture<D3D11_SHADER_INPUT_BIND_DESC> {
    let mut desc = D3D11_SHADER_INPUT_BIND_DESC::default();
    win(
        unsafe { shader.GetResourceBindingDesc(index, &mut desc) },
        "resource binding",
    )?;
    Ok(desc)
}

fn signature(
    shader: &ID3D11ShaderReflection,
    index: u32,
    input: bool,
) -> Fixture<D3D11_SIGNATURE_PARAMETER_DESC> {
    let mut desc = D3D11_SIGNATURE_PARAMETER_DESC::default();
    let status = unsafe {
        if input {
            shader.GetInputParameterDesc(index, &mut desc)
        } else {
            shader.GetOutputParameterDesc(index, &mut desc)
        }
    };
    win(status, "shader signature")?;
    Ok(desc)
}

struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    kind: &'static str,
}

impl Gpu {
    fn new() -> Fixture<Self> {
        let create = |driver, flags| {
            let (device, context) = win(
                rsf_test_support::d3d11_device(driver, flags, None),
                "D3D11CreateDevice",
            )?;
            let level = unsafe { device.GetFeatureLevel() };
            Ok::<_, String>((device, context, level))
        };
        let (kind, (device, context, level)) =
            match create(D3D_DRIVER_TYPE_HARDWARE, D3D11_CREATE_DEVICE_DEBUG) {
                Ok(result) => ("hardware", result),
                Err(_) => (
                    "WARP",
                    create(D3D_DRIVER_TYPE_WARP, D3D11_CREATE_DEVICE_FLAG(0))?,
                ),
            };
        check(
            level.0 >= D3D_FEATURE_LEVEL_11_0.0,
            "feature level 11 required",
        )?;
        Ok(Self {
            device,
            context,
            kind,
        })
    }

    fn compile(&self, source: &str, target: &CStr) -> Fixture<Vec<u8>> {
        compile_system_checked(Request {
            source: source.as_bytes(),
            name: c"first-party-fixture",
            entry: c"main",
            target,
            flags: D3DCOMPILE_ENABLE_STRICTNESS,
        })
        .map_err(|reason| format!("fixture {reason}"))
    }

    fn errors(&self) -> Fixture<()> {
        match rsf_test_support::device::d3d11_errors(&self.device)
            .into_iter()
            .next()
        {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn buffer(
        &self,
        desc: &D3D11_BUFFER_DESC,
        init: Option<&D3D11_SUBRESOURCE_DATA>,
    ) -> Fixture<ID3D11Buffer> {
        created("CreateBuffer", |out| unsafe {
            self.device
                .CreateBuffer(desc, init.map(ptr::from_ref), Some(out))
        })
    }

    fn upload<T>(&self, values: &[T], bind: D3D11_BIND_FLAG) -> Fixture<ID3D11Buffer> {
        self.buffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: size_of_val(values) as u32,
                BindFlags: bind.0 as u32,
                ..Default::default()
            },
            Some(&D3D11_SUBRESOURCE_DATA {
                pSysMem: values.as_ptr().cast(),
                ..Default::default()
            }),
        )
    }

    fn readback<T: Copy>(&self, buffer: &ID3D11Buffer, count: usize) -> Fixture<Vec<T>> {
        let staging = self.buffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: (count * size_of::<T>()) as u32,
                Usage: D3D11_USAGE_STAGING,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..Default::default()
            },
            None,
        )?;
        unsafe {
            self.context.CopyResource(&staging, buffer);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            win(
                self.context
                    .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)),
                "map readback",
            )?;
            let values = (!mapped.pData.is_null())
                .then(|| slice::from_raw_parts(mapped.pData.cast::<T>(), count).to_vec());
            self.context.Unmap(&staging, 0);
            values.ok_or_else(|| "readback map returned no data".into())
        }
    }
}

impl Gpu {
    fn constants(
        &self,
        shader: &ID3D11ShaderReflection,
        vertex: bool,
        hud: bool,
        history: V4,
    ) -> Fixture<()> {
        for index in 0..description(shader)?.BoundResources {
            let binding = binding(shader, index)?;
            if binding.Type != D3D_SIT_CBUFFER {
                continue;
            }
            let buffer = unsafe { shader.GetConstantBufferByName(binding.Name) }
                .ok_or("reflected constant buffer missing")?;
            let mut desc = D3D11_SHADER_BUFFER_DESC::default();
            win(
                unsafe { buffer.GetDesc(&mut desc) },
                "constant buffer reflection",
            )?;
            let mut values = vec![0.1; (desc.Size as usize).div_ceil(16) * 4];
            for index in 0..desc.Variables {
                let variable =
                    unsafe { buffer.GetVariableByIndex(index) }.ok_or("missing variable")?;
                let mut desc = D3D11_SHADER_VARIABLE_DESC::default();
                win(
                    unsafe { variable.GetDesc(&mut desc) },
                    "variable reflection",
                )?;
                let ty = unsafe { variable.GetType() }.ok_or("missing variable type")?;
                let mut shape = D3D11_SHADER_TYPE_DESC::default();
                win(
                    unsafe { ty.GetDesc(&mut shape) },
                    "variable type reflection",
                )?;
                check(desc.StartOffset % 4 == 0, "unaligned constant variable")?;
                let values = values
                    .get_mut(desc.StartOffset as usize / 4..)
                    .ok_or("constant offset out of bounds")?;
                let write = |target: &mut [f32], source: &[f32]| -> Fixture<()> {
                    target
                        .get_mut(..source.len())
                        .ok_or("constant data out of bounds")?
                        .copy_from_slice(source);
                    Ok(())
                };
                let label = name(desc.Name)?;
                match label.as_str() {
                    "rsf_previous_wvp" => write(values, matrix(true, hud).as_flattened())?,
                    "rsf_previous_bones" | "sbones_array" => {
                        write(
                            values,
                            palette(label == "rsf_previous_bones").as_flattened(),
                        )?;
                    }
                    "rsf_history" => write(values, &history)?,
                    "rsf_viewport" => write(values, &[0.0, 0.0, 1.0 / 16.0, 1.0 / 16.0])?,
                    "rsf_jitter" => write(values, &[0.01, -0.02, -0.03, 0.04])?,
                    "rsf_pixel_control" => write(values, &[1.0])?,
                    "alpha_ref" | "def_aref" => write(values, &[0.5])?,
                    _ if matches!(shape.Class, D3D_SVC_MATRIX_COLUMNS | D3D_SVC_MATRIX_ROWS) => {
                        let mut transform = [[0.0; 4]; 4];
                        for (i, row) in transform.iter_mut().enumerate() {
                            row[i] = 1.0;
                        }
                        if label == "m_WVP" {
                            transform = matrix(false, false);
                        }
                        check(
                            shape.Rows <= 4 && shape.Columns <= 4,
                            "unexpected matrix dimensions",
                        )?;
                        for (row, components) in
                            transform.iter().enumerate().take(shape.Rows as usize)
                        {
                            for (col, &value) in
                                components.iter().enumerate().take(shape.Columns as usize)
                            {
                                let index = if shape.Class == D3D_SVC_MATRIX_COLUMNS {
                                    col * 4 + row
                                } else {
                                    row * 4 + col
                                };
                                *values.get_mut(index).ok_or("matrix data out of bounds")? = value;
                            }
                        }
                    }
                    _ => {}
                }
            }
            check(
                (binding.BindPoint == 13) == name(binding.Name)?.starts_with("RSFCore"),
                "b13 collision",
            )?;
            let slot = [Some(self.upload(&values, D3D11_BIND_CONSTANT_BUFFER)?)];
            unsafe {
                if vertex {
                    self.context
                        .VSSetConstantBuffers(binding.BindPoint, Some(&slot));
                } else {
                    self.context
                        .PSSetConstantBuffers(binding.BindPoint, Some(&slot));
                }
            }
        }
        Ok(())
    }

    fn run_vs(
        &self,
        code: &[u8],
        original: &[D3D11_SIGNATURE_PARAMETER_DESC],
        mode: &str,
        augmented: bool,
        hud: bool,
        history: V4,
    ) -> Fixture<Vec<f32>> {
        let shader = reflect(code)?;
        let mut layout = Vec::new();
        let mut vertices = Vec::new();
        let formats = [
            [
                DXGI_FORMAT_R32_UINT,
                DXGI_FORMAT_R32G32_UINT,
                DXGI_FORMAT_R32G32B32_UINT,
                DXGI_FORMAT_R32G32B32A32_UINT,
            ],
            [
                DXGI_FORMAT_R32_SINT,
                DXGI_FORMAT_R32G32_SINT,
                DXGI_FORMAT_R32G32B32_SINT,
                DXGI_FORMAT_R32G32B32A32_SINT,
            ],
            [
                DXGI_FORMAT_R32_FLOAT,
                DXGI_FORMAT_R32G32_FLOAT,
                DXGI_FORMAT_R32G32B32_FLOAT,
                DXGI_FORMAT_R32G32B32A32_FLOAT,
            ],
        ];
        for index in 0..description(&shader)?.InputParameters {
            let input = signature(&shader, index, true)?;
            let count = input.Mask.count_ones() as usize;
            check((1..=4).contains(&count), "invalid vertex input mask")?;
            let category = match input.ComponentType {
                D3D_REGISTER_COMPONENT_UINT32 => 0,
                D3D_REGISTER_COMPONENT_SINT32 => 1,
                _ => 2,
            };
            layout.push(D3D11_INPUT_ELEMENT_DESC {
                SemanticName: input.SemanticName,
                SemanticIndex: input.SemanticIndex,
                Format: formats[category][count - 1],
                AlignedByteOffset: index * 16,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                ..Default::default()
            });
            let mut vertex = [0.4, 0.5, 0.6, 0.2];
            match name(input.SemanticName)?.as_str() {
                "POSITION" => vertex = [0.1, -0.2, 0.3, 0.7],
                "NORMAL" if mode == "1" => vertex[3] = 3.0 / 255.0,
                "TANGENT" => vertex[3] = 0.3,
                "BINORMAL" => vertex[3] = if mode == "3" { 9.0 / 255.0 } else { 0.1 },
                "TEXCOORD" if category != 2 => vertex = [32, 64, 3, 6].map(f32::from_bits),
                "TEXCOORD" if input.SemanticIndex == 1 => {
                    vertex = [12.0, 9.0, 6.0, 3.0].map(|value| value / 255.0);
                }
                "TEXCOORD" => vertex = [0.1, 0.2, 3.0, 6.0],
                _ => {}
            }
            vertices.push(vertex);
        }

        let mut so = Vec::new();
        let mut floats = 0;
        for output in original {
            let count = output.Mask.count_ones();
            so.push(D3D11_SO_DECLARATION_ENTRY {
                SemanticName: output.SemanticName,
                SemanticIndex: output.SemanticIndex,
                ComponentCount: count as u8,
                ..Default::default()
            });
            floats += count;
        }
        if augmented {
            for (index, count) in [(12, 4), (13, 1)] {
                so.push(D3D11_SO_DECLARATION_ENTRY {
                    SemanticName: PCSTR(c"TEXCOORD".as_ptr().cast()),
                    SemanticIndex: index,
                    ComponentCount: count,
                    ..Default::default()
                });
            }
            floats += 5;
        }
        let so_stride = floats * 4;
        let capture = self.buffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: so_stride,
                BindFlags: D3D11_BIND_STREAM_OUTPUT.0 as u32,
                ..Default::default()
            },
            None,
        )?;
        let context = &self.context;
        unsafe {
            context.ClearState();
            let input = created("CreateInputLayout", |out| {
                self.device.CreateInputLayout(&layout, code, Some(out))
            })?;
            let vb = Some(self.upload(&vertices, D3D11_BIND_VERTEX_BUFFER)?);
            let stride = (vertices.len() * 16) as u32;
            let offset = 0;
            context.IASetVertexBuffers(0, 1, Some(&vb), Some(&stride), Some(&offset));
            context.IASetInputLayout(&input);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_POINTLIST);
            let vs = created("CreateVertexShader", |out| {
                self.device.CreateVertexShader(code, NO_LINKAGE, Some(out))
            })?;
            context.VSSetShader(&vs, None);
            self.constants(&shader, true, hud, history)?;
            // D3D11 accepts the tested VS bytecode directly as a stream-output declaration.
            let gs = created("CreateGeometryShaderWithStreamOutput", |out| {
                self.device.CreateGeometryShaderWithStreamOutput(
                    code,
                    Some(&so),
                    Some(&[so_stride]),
                    D3D11_SO_NO_RASTERIZED_STREAM,
                    NO_LINKAGE,
                    Some(out),
                )
            })?;
            context.GSSetShader(&gs, None);
            context.SOSetTargets(1, Some(&Some(capture.clone())), Some(&offset));
            context.Draw(1, 0);
            context.SOSetTargets(0, None, None);
        }
        self.readback(&capture, floats as usize)
    }
}

fn pixel_fixture_vs(
    shader: &ID3D11ShaderReflection,
    valid: f32,
    previous_w: f32,
    stationary: bool,
) -> Fixture<String> {
    use std::fmt::Write;
    let mut declaration = String::from("struct O{");
    let mut assignments = String::new();
    for index in 0..description(shader)?.InputParameters {
        let input = signature(shader, index, true)?;
        if input.SystemValueType == D3D_NAME_POSITION {
            continue;
        }
        let semantic = name(input.SemanticName)?;
        let index_semantic = input.SemanticIndex;
        let count = input.Mask.count_ones() as usize;
        let mask = "xyzw"
            .get(..count)
            .filter(|mask| !mask.is_empty())
            .ok_or("invalid PS input mask")?;
        let interpolation = if semantic == "TEXCOORD" && index_semantic == 13 {
            "nointerpolation "
        } else {
            ""
        };
        write!(
            declaration,
            "{interpolation}float{count} f{index}:{semantic}{index_semantic};"
        )
        .unwrap();
        let value = match (semantic.as_str(), index_semantic) {
            ("TEXCOORD", 1) => "float4(0.1,0.2,2,0.7)".into(),
            ("TEXCOORD", 2) => "float4(0,0,1,0)".into(),
            ("TEXCOORD", 3) => "float4(0,1,0,0)".into(),
            ("TEXCOORD", 4) => "float4(1,0,0,0)".into(),
            ("TEXCOORD", 12) => {
                let xy = if stationary {
                    "(p.x-0.08)*w,(p.y-0.12)*w"
                } else {
                    "(p.x-0.2)*w,(p.y+0.4)*w"
                };
                format!("float4({xy},0.5*w,{previous_w:.6}*w)")
            }
            ("TEXCOORD", 13) => format!("float4({valid:.6},0,0,0)"),
            _ => "float4(0.5,0.5,0.5,0.5)".into(),
        };
        write!(assignments, "o.f{index}=({value}).{mask};").unwrap();
    }
    Ok(format!(
        "{declaration}float4 p:SV_Position;}};O main(uint id:SV_VertexID){{float2 p=float2((id==2)?3:-1,(id==1)?3:-1);float w=1+id;O o;o.p=float4(p*w,0.5*w,w);{assignments}return o;}}"
    ))
}

struct Target {
    rtv: ID3D11RenderTargetView,
    srv: ID3D11ShaderResourceView,
}

#[derive(Clone, Copy)]
struct PixelDraw {
    augmented: bool,
    validity: f32,
    previous_w: f32,
    alpha: f32,
    stationary: bool,
}

impl Default for PixelDraw {
    fn default() -> Self {
        Self {
            augmented: true,
            validity: 1.0,
            previous_w: 1.0,
            alpha: 1.0,
            stationary: false,
        }
    }
}

impl Gpu {
    fn target(&self, format: DXGI_FORMAT, samples: u32) -> Fixture<Target> {
        let desc = D3D11_TEXTURE2D_DESC {
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: samples,
                Quality: 0,
            },
            ..Device::desc(
                [WIDTH; 2],
                format,
                D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE,
            )
        };
        unsafe {
            let texture = created("MRT texture", |out| {
                self.device.CreateTexture2D(&desc, None, Some(out))
            })?;
            Ok(Target {
                rtv: created("MRT RTV", |out| {
                    self.device
                        .CreateRenderTargetView(&texture, None, Some(out))
                })?,
                srv: created("MRT SRV", |out| {
                    self.device
                        .CreateShaderResourceView(&texture, None, Some(out))
                })?,
            })
        }
    }

    fn samples(&self, target: Target, samples: u32) -> Fixture<Vec<V4>> {
        let count = PIXELS * samples;
        let (declaration, load) = if samples == 1 {
            (
                "Texture2D<float4> t:register(t0);".into(),
                "t.Load(int3(id.xy,0))",
            )
        } else {
            (
                format!("Texture2DMS<float4,{samples}> t:register(t0);"),
                "t.Load(id.xy,s)",
            )
        };
        let source = format!(
            "{declaration}RWStructuredBuffer<float4> outp:register(u0);[numthreads(16,16,1)]void main(uint3 id:SV_DispatchThreadID){{for(uint s=0;s<{samples};++s)outp[(id.y*16+id.x)*{samples}+s]={load};}}"
        );
        let code = self.compile(&source, c"cs_5_0")?;
        let output = self.buffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: count * 16,
                BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
                MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
                StructureByteStride: 16,
                ..Default::default()
            },
            None,
        )?;
        unsafe {
            let shader = created("readback CS", |out| {
                self.device
                    .CreateComputeShader(&code, NO_LINKAGE, Some(out))
            })?;
            let uav = created("readback UAV", |out| {
                self.device
                    .CreateUnorderedAccessView(&output, None, Some(out))
            })?;
            self.context.CSSetShader(&shader, None);
            self.context
                .CSSetShaderResources(0, Some(&[Some(target.srv)]));
            self.context
                .CSSetUnorderedAccessViews(0, 1, Some(&Some(uav)), None);
            self.context.Dispatch(1, 1, 1);
            self.context.CSSetShaderResources(0, Some(&[None]));
            self.context
                .CSSetUnorderedAccessViews(0, 1, Some(&None), None);
        }
        let result = self.readback(&output, count as usize)?;
        self.errors()?;
        Ok(result)
    }

    fn materials(&self, shader: &ID3D11ShaderReflection, alpha: f32) -> Fixture<()> {
        for index in 0..description(shader)?.BoundResources {
            let binding = binding(shader, index)?;
            unsafe {
                match binding.Type {
                    D3D_SIT_TEXTURE => {
                        check(
                            binding.Dimension == D3D_SRV_DIMENSION_TEXTURE2D,
                            "unexpected material resource dimension",
                        )?;
                        let desc = Device::desc(
                            [2, 2],
                            DXGI_FORMAT_R32G32B32A32_FLOAT,
                            D3D11_BIND_SHADER_RESOURCE,
                        );
                        let alpha = if name(binding.Name)? == "s_base" {
                            alpha
                        } else {
                            1.0
                        };
                        let texels = [[0.5_f32, 0.4, 0.6, alpha]; 4];
                        let init = D3D11_SUBRESOURCE_DATA {
                            pSysMem: texels.as_ptr().cast(),
                            SysMemPitch: 32,
                            ..Default::default()
                        };
                        let texture = created("fixture texture", |out| {
                            self.device.CreateTexture2D(&desc, Some(&init), Some(out))
                        })?;
                        let srv = created("fixture texture SRV", |out| {
                            self.device
                                .CreateShaderResourceView(&texture, None, Some(out))
                        })?;
                        self.context
                            .PSSetShaderResources(binding.BindPoint, Some(&[Some(srv)]));
                    }
                    D3D_SIT_SAMPLER => {
                        let desc = D3D11_SAMPLER_DESC {
                            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                            MaxLOD: D3D11_FLOAT32_MAX,
                            ..Default::default()
                        };
                        let sampler = created("fixture sampler", |out| {
                            self.device.CreateSamplerState(&desc, Some(out))
                        })?;
                        self.context
                            .PSSetSamplers(binding.BindPoint, Some(&[Some(sampler)]));
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn run_ps(&self, code: &[u8], samples: u32, draw: PixelDraw) -> Fixture<Pixels> {
        let reflection = reflect(code)?;
        let source =
            pixel_fixture_vs(&reflection, draw.validity, draw.previous_w, draw.stationary)?;
        let vertex_code = self.compile(&source, c"vs_5_0")?;
        let targets = [
            self.target(DXGI_FORMAT_R32G32B32A32_FLOAT, samples)?,
            self.target(DXGI_FORMAT_R32G32B32A32_FLOAT, samples)?,
            self.target(DXGI_FORMAT_R16G16B16A16_FLOAT, samples)?,
        ];
        let context = &self.context;
        unsafe {
            context.ClearState();
            let vs = created("fixture VS", |out| {
                self.device
                    .CreateVertexShader(&vertex_code, NO_LINKAGE, Some(out))
            })?;
            let ps = created("real PS", |out| {
                self.device.CreatePixelShader(code, NO_LINKAGE, Some(out))
            })?;
            context.VSSetShader(&vs, None);
            context.PSSetShader(&ps, None);
            self.constants(&reflection, false, false, HISTORY)?;
            self.materials(&reflection, draw.alpha)?;
            context.ClearRenderTargetView(&targets[0].rtv, &[SENTINEL; 4]);
            context.ClearRenderTargetView(&targets[1].rtv, &[SENTINEL; 4]);
            context.ClearRenderTargetView(&targets[2].rtv, &[0.0; 4]);
            let views = std::array::from_fn::<_, 3, _>(|i| Some(targets[i].rtv.clone()));
            context
                .OMSetRenderTargets(Some(&views[..if draw.augmented { 3 } else { 2 }]), NO_DEPTH);
            let rasterizer = D3D11_RASTERIZER_DESC {
                FillMode: D3D11_FILL_SOLID,
                CullMode: D3D11_CULL_NONE,
                DepthClipEnable: true.into(),
                MultisampleEnable: (samples > 1).into(),
                ..Default::default()
            };
            let rasterizer = created("fixture rasterizer", |out| {
                self.device.CreateRasterizerState(&rasterizer, Some(out))
            })?;
            context.RSSetState(&rasterizer);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                Width: WIDTH as f32,
                Height: WIDTH as f32,
                MaxDepth: 1.0,
                ..Default::default()
            }]));
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.Draw(3, 0);
            context.OMSetRenderTargets(None, NO_DEPTH);
        }
        let [a, b, motion] = targets;
        Ok([
            self.samples(a, samples)?,
            self.samples(b, samples)?,
            self.samples(motion, samples)?,
        ])
    }
}

#[derive(Default)]
struct Counts {
    vertex: usize,
    pixel: usize,
    invalid: usize,
    stationary: usize,
}

fn check_vs(
    gpu: &Gpu,
    stock: &[u8],
    variant: &[u8],
    mode: &str,
    counts: &mut Counts,
) -> Fixture<()> {
    let shader = reflect(stock)?;
    let outputs: Vec<_> = (0..description(&shader)?.OutputParameters)
        .map(|index| signature(&shader, index, false))
        .collect::<Fixture<_>>()?;
    let before = gpu.run_vs(stock, &outputs, mode, false, false, HISTORY)?;
    let current = mul(matrix(false, false), deformed(mode, false)?);
    for hud in [false, true] {
        let after = gpu.run_vs(variant, &outputs, mode, true, hud, HISTORY)?;
        check(after.len() == before.len() + 5, "VS stream size")?;
        for (&a, &b) in after.iter().zip(&before) {
            exact(a, b, "native output unchanged")?;
        }
        let prior = mul(matrix(true, hud), deformed(mode, true)?);
        let mut index = 0;
        for output in &outputs {
            if output.SystemValueType == D3D_NAME_POSITION {
                for (&actual, expected) in after[index..index + 4].iter().zip(current) {
                    exact(actual, expected, "CPU current clip")?;
                }
            }
            index += output.Mask.count_ones() as usize;
        }
        for (&actual, expected) in after[before.len()..].iter().zip(prior) {
            exact(actual, expected, "CPU prior clip")?;
        }
        exact(after[before.len() + 4], 1.0, "valid prior")?;
        counts.vertex += 1;
    }
    let history = match mode {
        "STATIC" | "0" => [0.0, 1.0, 1.0, 1.0],
        "NONE" => [1.0, 1.0, 0.0, 1.0],
        _ => [1.0, 0.0, 1.0, 1.0],
    };
    let invalid = gpu.run_vs(variant, &outputs, mode, true, false, history)?;
    exact(
        *invalid.last().ok_or("missing prior result")?,
        0.0,
        "missing prior invalid",
    )?;
    counts.invalid += 1;
    Ok(())
}

fn check_ps(
    gpu: &Gpu,
    stock: &[u8],
    variant: &[u8],
    samples: u32,
    alpha_test: u32,
    counts: &mut Counts,
) -> Fixture<()> {
    let alphas: &[f32] = if alpha_test == 0 {
        &[1.0]
    } else {
        &[1.0, 0.45, 0.0]
    };
    for &alpha in alphas {
        let draw = PixelDraw {
            alpha,
            ..Default::default()
        };
        let before = gpu.run_ps(
            stock,
            samples,
            PixelDraw {
                augmented: false,
                ..draw
            },
        )?;
        let after = gpu.run_ps(variant, samples, draw)?;
        let mut covered = 0;
        for (index, motion) in after[2].iter().enumerate() {
            for (original, augmented) in before[..2].iter().zip(&after[..2]) {
                for (a, b) in augmented[index].into_iter().zip(original[index]) {
                    // Native packed hemi/material deliberately reinterprets bits as float.
                    if a.to_bits() != b.to_bits() {
                        exact(a, b, "native MRT/coverage unchanged")?;
                    }
                }
            }
            if before[0][index][2] == SENTINEL {
                for &component in motion {
                    exact(component, 0.0, "uncovered sample motion")?;
                }
                continue;
            }
            if motion[2] != 1.0 {
                return Err(format!(
                    "motion invalid at pixel {index} alpha {alpha} vec={motion:?}"
                ));
            }
            close(motion[0], 0.06, 0.0003, "UV X motion/jitter")?;
            close(motion[1], 0.26, 0.0003, "UV Y down motion/jitter")?;
            exact(motion[2], 1.0, "motion validity")?;
            covered += 1;
        }
        check(
            alpha != 1.0 || covered > 0,
            "fixture produced no covered samples",
        )?;
        check(
            alpha_test != 1 || alpha != 0.0 || covered == 0,
            "alpha rejection fixture failed",
        )?;
        counts.pixel += 1;
    }
    let stationary = gpu.run_ps(
        variant,
        samples,
        PixelDraw {
            stationary: true,
            ..Default::default()
        },
    )?;
    let mut covered = 0;
    for (native, motion) in stationary[0].iter().zip(&stationary[2]) {
        if native[2] == SENTINEL {
            continue;
        }
        close(motion[0], 0.0, 0.0003, "stationary jitter cancellation X")?;
        close(motion[1], 0.0, 0.0003, "stationary jitter cancellation Y")?;
        exact(motion[2], 1.0, "zero motion is valid coverage")?;
        exact(motion[3], 0.0, "reserved motion W")?;
        covered += 1;
    }
    check(covered > 0, "stationary fixture has no coverage")?;
    counts.stationary += 1;
    for (validity, previous_w) in [(0.0, 1.0), (1.0, -1.0)] {
        let invalid = gpu.run_ps(
            variant,
            samples,
            PixelDraw {
                validity,
                previous_w,
                ..Default::default()
            },
        )?;
        for motion in &invalid[2] {
            for &component in motion {
                exact(component, 0.0, "invalid prior coverage")?;
            }
        }
        counts.invalid += 1;
    }
    Ok(())
}

fn run_case(gpu: &Gpu, root: &Path, fields: [&str; 5], counts: &mut Counts) -> Fixture<()> {
    let [index, stage, mode, samples, alpha] = fields;
    let index: u32 = index.parse().map_err(|_| "invalid fixture index")?;
    let samples: u32 = samples.parse().map_err(|_| "invalid MSAA count")?;
    let alpha: u32 = alpha.parse().map_err(|_| "invalid alpha mode")?;
    check(
        matches!(samples, 1 | 2 | 4 | 8) && alpha <= 2,
        "unsupported sample or alpha mode",
    )?;
    let read = |kind| {
        let path = root.join(format!("{index}-{kind}.dxbc"));
        fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))
    };
    let (stock, variant) = (read("old")?, read("new")?);
    match stage {
        "vs" => check_vs(gpu, &stock, &variant, mode, counts),
        "ps" => check_ps(gpu, &stock, &variant, samples, alpha, counts),
        _ => Err(format!("unexpected fixture stage {stage}")),
    }
}

pub(super) fn run() -> Fixture<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let [root, listing] = args.as_slice() else {
        return Err("usage: fixture DXBC_DIR CASES_TSV".into());
    };
    let listing = fs::read_to_string(listing).map_err(|error| format!("case list: {error}"))?;
    let gpu = Gpu::new()?;
    let mut counts = Counts::default();
    for (line, text) in listing.lines().enumerate() {
        let fields: Vec<_> = text.split_whitespace().collect();
        let fields = fields
            .try_into()
            .map_err(|_| format!("case list line {}: expected five fields", line + 1))?;
        run_case(&gpu, Path::new(root), fields, &mut counts)
            .map_err(|error| format!("case {}: {error}", fields[0]))?;
    }
    println!(
        "{{\"device\":\"{}\",\"vs_checks\":{},\"ps_checks\":{},\"invalid_checks\":{},\"zero_motion_checks\":{}}}",
        gpu.kind, counts.vertex, counts.pixel, counts.invalid, counts.stationary
    );
    Ok(())
}
