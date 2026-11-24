// SPDX-License-Identifier: MIT
//! Runs local FXC-built foliage variants through D3D11 stream output and MRTs.
#[cfg(windows)]
mod fixture {
    use rsf_shader_compiler::{Request, compile_system_checked};
    use rsf_test_support::device::{Device, half_to_f32 as half};
    use std::ffi::{CStr, CString};
    use std::fmt::Write;
    use std::path::Path;
    use std::ops::Deref;
    use std::ptr::{from_ref, null_mut, read_unaligned};
    use windows::Win32::Graphics::Direct3D::Fxc::D3DReflect;
    use windows::Win32::Graphics::Direct3D::*;
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::Dxgi::{Common::*, IDXGIDevice};
    use windows::core::{Interface, PCSTR};

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    fn require(condition: bool, message: impl Into<String>) -> Result<()> {
        if condition {
            Ok(())
        } else {
            Err(message.into().into())
        }
    }

    fn read(path: &Path) -> Result<Vec<u8>> {
        std::fs::read(path).map_err(|error| format!("read DXBC {}: {error}", path.display()).into())
    }

    fn compile_fixture(source: &str, profile: &CStr) -> Result<Vec<u8>> {
        compile_system_checked(Request {
            source: source.as_bytes(),
            name: c"first-party-fixture",
            entry: c"main",
            target: profile,
            flags: 1 << 11, // D3DCOMPILE_ENABLE_STRICTNESS
        })
        .map_err(|reason| format!("D3DCompile fixture: {reason}").into())
    }

    struct Constant {
        name: CString,
        slot: u32,
        data: Vec<u8>,
        buffer: ID3D11Buffer,
    }

    struct Shader {
        dxbc: Vec<u8>,
        shader: ID3D11VertexShader,
        reflection: ID3D11ShaderReflection,
        constants: Vec<Constant>,
    }

    impl Shader {
        fn new(device: &Gpu, path: &Path) -> Result<Self> {
            let dxbc = read(path)?;
            let mut shader = None;
            let mut reflected = null_mut();
            unsafe {
                device.device.CreateVertexShader(
                    &dxbc,
                    None::<&ID3D11ClassLinkage>,
                    Some(&mut shader),
                )?;
                D3DReflect(
                    dxbc.as_ptr().cast(),
                    dxbc.len(),
                    &ID3D11ShaderReflection::IID,
                    &mut reflected,
                )?;
            }
            require(!reflected.is_null(), "D3DReflect returned null")?;
            let reflection = unsafe { ID3D11ShaderReflection::from_raw(reflected) };
            let shader = shader.ok_or("CreateVertexShader returned null")?;
            let mut desc = D3D11_SHADER_DESC::default();
            unsafe { reflection.GetDesc(&mut desc) }?;
            let mut constants = Vec::new();
            for index in 0..desc.ConstantBuffers {
                let mut buffer_desc = D3D11_SHADER_BUFFER_DESC::default();
                let constant = unsafe { reflection.GetConstantBufferByIndex(index) }
                    .ok_or("constant buffer reflection")?;
                unsafe { constant.GetDesc(&mut buffer_desc) }?;
                let mut binding = D3D11_SHADER_INPUT_BIND_DESC::default();
                unsafe { reflection.GetResourceBindingDescByName(buffer_desc.Name, &mut binding) }?;
                require(
                    !buffer_desc.Name.is_null(),
                    "constant buffer name unavailable",
                )?;
                let name = unsafe { CStr::from_ptr(buffer_desc.Name.0.cast()) }.to_owned();
                if name.as_c_str() == c"RSFFoliagePrevious" {
                    require(
                        buffer_desc.Size == 5056 && binding.BindPoint == 13,
                        "reflected foliage ABI size/slot mismatch",
                    )?;
                }
                let size = buffer_desc.Size.next_multiple_of(16);
                let buffer = device.buffer(size, D3D11_BIND_CONSTANT_BUFFER.0 as u32, None)?;
                constants.push(Constant {
                    name,
                    slot: binding.BindPoint,
                    data: vec![0; size as usize],
                    buffer,
                });
            }
            Ok(Self {
                dxbc,
                shader,
                reflection,
                constants,
            })
        }

        fn variable(
            reflection: &ID3D11ShaderReflection,
            buffer: &Constant,
            name: &CStr,
        ) -> Option<(ID3D11ShaderReflectionVariable, D3D11_SHADER_VARIABLE_DESC)> {
            let constant =
                unsafe { reflection.GetConstantBufferByName(PCSTR(buffer.name.as_ptr().cast())) }?;
            let variable = unsafe { constant.GetVariableByName(PCSTR(name.as_ptr().cast())) }?;
            let mut desc = D3D11_SHADER_VARIABLE_DESC::default();
            unsafe { variable.GetDesc(&mut desc) }.ok()?;
            Some((variable, desc))
        }

        fn set(&mut self, name: &CStr, words: impl IntoIterator<Item = [u8; 4]>) -> Result<()> {
            for buffer in &mut self.constants {
                let Some((_, desc)) = Self::variable(&self.reflection, buffer, name) else {
                    continue;
                };
                let start = desc.StartOffset as usize;
                let size = desc.Size as usize;
                let dst = buffer
                    .data
                    .get_mut(start..start + size)
                    .ok_or("constant extent outside buffer")?;
                for (i, word) in words.into_iter().enumerate() {
                    let offset = i * 4;
                    dst.get_mut(offset..offset + 4)
                        .ok_or_else(|| format!("oversized variable {name:?}"))?
                        .copy_from_slice(&word);
                }
                break;
            }
            Ok(())
        }

        fn floats(&mut self, name: &CStr, data: &[f32]) -> Result<()> {
            self.set(name, data.iter().copied().map(f32::to_ne_bytes))
        }

        fn matrix(&mut self, name: &CStr, scale: f32, tx: f32, ty: f32) -> Result<()> {
            for buffer in &mut self.constants {
                let Some((variable, desc)) = Self::variable(&self.reflection, buffer, name) else {
                    continue;
                };
                let mut ty_desc = D3D11_SHADER_TYPE_DESC::default();
                let ty_desc_source = unsafe { variable.GetType() }.ok_or("matrix type missing")?;
                unsafe { ty_desc_source.GetDesc(&mut ty_desc) }?;
                for row in 0..ty_desc.Rows {
                    for col in 0..ty_desc.Columns {
                        let index = if ty_desc.Class == D3D_SVC_MATRIX_ROWS {
                            row * 4 + col
                        } else {
                            col * 4 + row
                        };
                        let value = match (row, col) {
                            (0, 3) => tx,
                            (1, 3) => ty,
                            (3, 3) => 1.0,
                            (r, c) if r == c => scale,
                            _ => 0.0,
                        };
                        let offset = (desc.StartOffset + index * 4) as usize;
                        buffer
                            .data
                            .get_mut(offset..offset + 4)
                            .ok_or("matrix extent outside buffer")?
                            .copy_from_slice(&value.to_ne_bytes());
                    }
                }
                break;
            }
            Ok(())
        }

        fn bind(&self, context: &ID3D11DeviceContext) {
            let empty = [const { None }; 14];
            unsafe { context.VSSetConstantBuffers(0, Some(&empty)) };
            for buffer in &self.constants {
                unsafe {
                    context.UpdateSubresource(
                        &buffer.buffer,
                        0,
                        None,
                        buffer.data.as_ptr().cast(),
                        0,
                        0,
                    );
                    let binding = [Some(buffer.buffer.clone())];
                    context.VSSetConstantBuffers(buffer.slot, Some(&binding));
                }
            }
            unsafe { context.VSSetShader(&self.shader, None) };
        }

        fn signature(&self, prefix: &str) -> Result<Signature> {
            let mut desc = D3D11_SHADER_DESC::default();
            unsafe { self.reflection.GetDesc(&mut desc) }?;
            let mut result = Signature {
                declaration: "struct T {".into(),
                ..Default::default()
            };
            for index in 0..desc.OutputParameters {
                let mut parameter = D3D11_SIGNATURE_PARAMETER_DESC::default();
                unsafe {
                    self.reflection
                        .GetOutputParameterDesc(index, &mut parameter)
                }?;
                require(
                    !parameter.SemanticName.is_null(),
                    "output semantic name unavailable",
                )?;
                let semantic =
                    unsafe { CStr::from_ptr(parameter.SemanticName.0.cast()) }.to_str()?;
                let width = u8::BITS - parameter.Mask.leading_zeros();
                let field = format!("{prefix}{index}");
                let valid = semantic == "TEXCOORD" && parameter.SemanticIndex == 13;
                if valid {
                    result.declaration.push_str("nointerpolation ");
                }
                write!(
                    result.declaration,
                    "float{width} {field}:{semantic}{};",
                    parameter.SemanticIndex
                )?;
                if parameter.SystemValueType == D3D_NAME_POSITION {
                    result.current.clone_from(&field);
                }
                if semantic == "TEXCOORD" && parameter.SemanticIndex == 12 {
                    result.previous.clone_from(&field);
                }
                if valid {
                    result.valid = field;
                }
            }
            result.declaration.push_str("};");
            Ok(result)
        }
    }

    #[derive(Default)]
    struct Signature {
        declaration: String,
        current: String,
        previous: String,
        valid: String,
    }

    fn rows(prior: bool) -> [f32; 61 * 16] {
        let mut rows = [0.0; 61 * 16];
        for (i, row) in rows.chunks_exact_mut(16).enumerate() {
            let scale = if prior { 0.5 } else { 1.0 };
            row[0] = scale;
            row[5] = scale;
            row[10] = scale;
            row[3] = (if prior { -0.3 } else { 0.1 }) + i as f32 * 0.01;
            row[12] = 0.5;
            row[15] = 0.5;
        }
        rows
    }

    fn setup(shader: &mut Shader, prior: bool) -> Result<()> {
        for name in [c"m_V", c"m_WV", c"m_xform_v"] {
            shader.matrix(name, 1.0, 0.0, 0.0)?;
        }
        let (tx, ty) = if prior { (0.05, -0.04) } else { (0.0, 0.0) };
        for name in [c"m_VP", c"m_WVP"] {
            shader.matrix(name, 1.0, tx, ty)?;
        }
        let (scale, tx, ty) = if prior {
            (0.75, -0.2, 0.1)
        } else {
            (1.0, 0.1, 0.0)
        };
        shader.matrix(c"m_xform", scale, tx, ty)?;
        shader.floats(c"consts", &[if prior { 0.5 } else { 1.0 }, 1.0, 1.0, 1.0])?;
        shader.floats(c"wave", &[0.0, 0.0, 0.0, if prior { 0.0 } else { 0.5 }])?;
        let wind = if prior {
            [0.5, 0.0, 0.25, 0.0]
        } else {
            [0.25, 0.0, 0.125, 0.0]
        };
        for name in [c"wind", c"dir2D"] {
            shader.floats(name, &wind)?;
        }
        shader.floats(c"c_scale", &[1.0; 4])?;
        shader.floats(c"array", &rows(prior))
    }

    fn flags(shader: &mut Shader, valid: u32) -> Result<()> {
        shader.set(
            c"rsf_foliage_history",
            [valid, 61, 1, 0].map(u32::to_ne_bytes),
        )
    }

    fn history(shader: &mut Shader) -> Result<()> {
        shader.matrix(c"rsf_previous_clip_from_world", 1.0, 0.05, -0.04)?;
        shader.matrix(c"rsf_previous_tree_xform", 0.75, -0.2, 0.1)?;
        shader.floats(c"rsf_previous_consts", &[0.5, 1.0, 1.0, 1.0])?;
        shader.floats(c"rsf_previous_wave", &[0.0; 4])?;
        shader.floats(c"rsf_previous_wind", &[0.5, 0.0, 0.25, 0.0])?;
        flags(shader, 1)?;
        shader.floats(c"rsf_previous_rows", &rows(true))?;
        let mut state = [0.0; 61 * 4];
        for group in state.chunks_exact_mut(4) {
            group.copy_from_slice(&[1.0, 0.5, 0.75, 0.0]);
        }
        shader.floats(c"rsf_previous_item_state", &state)
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Vertex {
        position: [f32; 4],
        normal: [f32; 4],
        tangent: [f32; 3],
        binormal: [f32; 3],
        misc: [i32; 4],
    }

    impl Vertex {
        fn new(position: [f32; 4], row: i32) -> Self {
            Self {
                position,
                normal: [0.0, 1.0, 0.0, 1.0],
                tangent: [1.0, 0.0, 0.0],
                binormal: [0.0, 0.0, 1.0],
                misc: [0, 0, 1, row],
            }
        }
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Capture {
        current: [f32; 4],
        previous: [f32; 4],
        valid: f32,
    }

    const _: () = {
        assert!(size_of::<Vertex>() == 72);
        assert!(std::mem::offset_of!(Vertex, misc) == 56);
        assert!(size_of::<Capture>() == 36);
    };

    /// The support crate's device with this fixture's buffer, draw and raster passes.
    struct Gpu(Device);

    impl Deref for Gpu {
        type Target = Device;
        fn deref(&self) -> &Device {
            &self.0
        }
    }

    impl Gpu {
        fn new() -> Result<Self> {
            for (driver, name) in [
                (D3D_DRIVER_TYPE_HARDWARE, "hardware"),
                (D3D_DRIVER_TYPE_WARP, "WARP"),
            ] {
                let Some(gpu) = Device::create(driver, None) else {
                    continue;
                };
                let level = unsafe { gpu.device.GetFeatureLevel() };
                require(
                    level.0 >= D3D_FEATURE_LEVEL_11_0.0,
                    "feature level 11 required",
                )?;

                let dxgi = gpu.device.cast::<IDXGIDevice>()?;
                let adapter = unsafe { dxgi.GetAdapter()?.GetDesc()? };
                let name_end = adapter
                    .Description
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(adapter.Description.len());
                let adapter_name = String::from_utf16_lossy(&adapter.Description[..name_end]);
                println!("device={name} feature_level={}", level.0);
                println!(
                    "adapter={adapter_name} vendor={} device={}",
                    adapter.VendorId, adapter.DeviceId
                );
                return Ok(Self(gpu));
            }
            Err("D3D11CreateDevice: hardware and WARP refused".into())
        }

        fn buffer(
            &self,
            size: u32,
            flags: u32,
            data: Option<&D3D11_SUBRESOURCE_DATA>,
        ) -> Result<ID3D11Buffer> {
            let desc = D3D11_BUFFER_DESC {
                ByteWidth: size,
                BindFlags: flags,
                ..Default::default()
            };
            let mut out = None;
            unsafe {
                self.device
                    .CreateBuffer(&desc, data.map(from_ref), Some(&mut out))
            }?;
            out.ok_or_else(|| "CreateBuffer returned null".into())
        }

        fn vertices(&self, shader: &Shader, vertices: &[Vertex], detail: bool) -> Result<()> {
            let element = |semantic: &CStr, format, offset| D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(semantic.as_ptr().cast()),
                Format: format,
                AlignedByteOffset: offset,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                ..Default::default()
            };
            let mut elements = vec![element(c"POSITION", DXGI_FORMAT_R32G32B32A32_FLOAT, 0)];
            if !detail {
                elements.extend([
                    element(c"NORMAL", DXGI_FORMAT_R32G32B32A32_FLOAT, 16),
                    element(c"TANGENT", DXGI_FORMAT_R32G32B32_FLOAT, 32),
                    element(c"BINORMAL", DXGI_FORMAT_R32G32B32_FLOAT, 44),
                ]);
            }
            elements.push(element(c"TEXCOORD", DXGI_FORMAT_R32G32B32A32_SINT, 56));
            let mut layout = None;
            unsafe {
                self.device
                    .CreateInputLayout(&elements, &shader.dxbc, Some(&mut layout))
            }?;
            let layout = layout.ok_or("CreateInputLayout returned null")?;
            let initial = D3D11_SUBRESOURCE_DATA {
                pSysMem: vertices.as_ptr().cast(),
                ..Default::default()
            };
            let buffer = self.buffer(
                std::mem::size_of_val(vertices) as u32,
                D3D11_BIND_VERTEX_BUFFER.0 as u32,
                Some(&initial),
            )?;
            let binding = [Some(buffer)];
            unsafe {
                self.context.IASetInputLayout(&layout);
                self.context.IASetVertexBuffers(
                    0,
                    1,
                    Some(binding.as_ptr()),
                    Some(&(size_of::<Vertex>() as u32)),
                    Some(&0),
                );
            }
            Ok(())
        }

        fn draw(&self, shader: &Shader, changed: bool, detail: bool, row: i32) -> Result<Capture> {
            self.vertices(shader, &[Vertex::new([0.2, 0.4, 0.5, 1.0], row)], detail)?;
            let mut signature = shader.signature("field")?;
            let code = &mut signature.declaration;
            code.push_str(" struct U {float4 c:TEXCOORD10;");
            if changed {
                code.push_str("float4 p:TEXCOORD12; nointerpolation float v:TEXCOORD13;");
            }
            write!(
                code,
                "}};[maxvertexcount(1)] void main(point T i[1],inout PointStream<U> s){{U o;o.c=i[0].{};",
                signature.current
            )?;
            if changed {
                write!(
                    code,
                    "o.p=i[0].{};o.v=i[0].{};",
                    signature.previous, signature.valid
                )?;
            }
            code.push_str("s.Append(o);}");
            let blob = compile_fixture(code, c"gs_5_0")?;
            let declaration = [10, 12, 13].map(|index| D3D11_SO_DECLARATION_ENTRY {
                SemanticName: PCSTR(c"TEXCOORD".as_ptr().cast()),
                SemanticIndex: index,
                ComponentCount: if index == 13 { 1 } else { 4 },
                ..Default::default()
            });
            let stride = if changed {
                size_of::<Capture>() as u32
            } else {
                16
            };
            let mut gs = None;
            unsafe {
                self.device.CreateGeometryShaderWithStreamOutput(
                    &blob,
                    Some(&declaration[..if changed { 3 } else { 1 }]),
                    Some(&[stride]),
                    D3D11_SO_NO_RASTERIZED_STREAM,
                    None::<&ID3D11ClassLinkage>,
                    Some(&mut gs),
                )
            }?;
            let gs = gs.ok_or("stream output GS unavailable")?;
            let so = self.buffer(
                size_of::<Capture>() as u32,
                D3D11_BIND_STREAM_OUTPUT.0 as u32,
                None,
            )?;
            let desc = D3D11_BUFFER_DESC {
                ByteWidth: size_of::<Capture>() as u32,
                Usage: D3D11_USAGE_STAGING,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..Default::default()
            };
            let mut staging = None;
            unsafe { self.device.CreateBuffer(&desc, None, Some(&mut staging)) }?;
            let staging = staging.ok_or("stream output readback unavailable")?;
            let bound = [Some(so.clone())];
            let ctx = &self.context;
            unsafe {
                ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_POINTLIST);
                ctx.GSSetShader(&gs, None);
                ctx.SOSetTargets(1, Some(bound.as_ptr()), Some(&0));
                shader.bind(ctx);
                ctx.Draw(1, 0);
                ctx.SOSetTargets(0, None, None);
                ctx.CopyResource(&staging, &so);
            }
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            unsafe { ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }?;
            // The known SO declaration writes either the complete record or its current clip.
            let result = if mapped.pData.is_null() {
                None
            } else if changed {
                Some(unsafe { read_unaligned(mapped.pData.cast::<Capture>()) })
            } else {
                Some(Capture {
                    current: unsafe { read_unaligned(mapped.pData.cast::<[f32; 4]>()) },
                    ..Default::default()
                })
            };
            unsafe { ctx.Unmap(&staging, 0) };
            result.ok_or_else(|| "stream output Map returned null".into())
        }
    }

    impl Gpu {
        fn raster_state(&self) -> Result<()> {
            let rs = D3D11_RASTERIZER_DESC {
                FillMode: D3D11_FILL_SOLID,
                CullMode: D3D11_CULL_NONE,
                DepthClipEnable: true.into(),
                ..Default::default()
            };
            let ds = D3D11_DEPTH_STENCIL_DESC {
                DepthFunc: D3D11_COMPARISON_ALWAYS,
                ..Default::default()
            };
            let (mut rasterizer, mut depth) = (None, None);
            unsafe {
                self.device
                    .CreateRasterizerState(&rs, Some(&mut rasterizer))?;
                self.device.CreateDepthStencilState(&ds, Some(&mut depth))?;
                self.context.RSSetState(rasterizer.as_ref());
                self.context.OMSetDepthStencilState(depth.as_ref(), 0);
                self.context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                    Width: 16.0,
                    Height: 16.0,
                    MaxDepth: 1.0,
                    ..Default::default()
                }]));
            }
            Ok(())
        }

        fn pixel_inputs(&self) -> Result<()> {
            let ps_data = [
                0.0f32,
                0.0,
                1.0 / 16.0,
                1.0 / 16.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                0.0,
                0.0,
            ];
            let initial = D3D11_SUBRESOURCE_DATA {
                pSysMem: ps_data.as_ptr().cast(),
                ..Default::default()
            };
            let buffer = self.buffer(48, D3D11_BIND_CONSTANT_BUFFER.0 as u32, Some(&initial))?;
            let texel = u32::MAX;
            let initial = D3D11_SUBRESOURCE_DATA {
                pSysMem: from_ref(&texel).cast(),
                SysMemPitch: 4,
                ..Default::default()
            };
            let white = self.texture(
                &Device::desc([1, 1], DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_SHADER_RESOURCE),
                Some(&initial),
            )?;
            let mut srv = None;
            unsafe {
                self.device
                    .CreateShaderResourceView(&white, None, Some(&mut srv))
            }?;
            let mut sampler = None;
            let desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_POINT,
                AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                MaxLOD: f32::MAX,
                ..Default::default()
            };
            unsafe {
                self.device.CreateSamplerState(&desc, Some(&mut sampler))?;
                self.context.PSSetConstantBuffers(13, Some(&[Some(buffer)]));
                let views: [_; 16] = std::array::from_fn(|_| srv.clone());
                let samplers: [_; 16] = std::array::from_fn(|_| sampler.clone());
                self.context.PSSetShaderResources(0, Some(&views));
                self.context.PSSetSamplers(0, Some(&samplers));
            }
            Ok(())
        }

        fn raster(
            &self,
            shader: &Shader,
            pixel_path: &Path,
            detail: bool,
            diagnostic: bool,
        ) -> Result<[[f32; 4]; 3]> {
            let pixel = if diagnostic {
                let signature = shader.signature("f")?;
                let code = format!(
                    "{}struct R {{float4 p:SV_Target0;float4 v:SV_Target1;}};R main(T i){{R o;o.p=i.{};o.v=float4(i.{},0,0,0);return o;}}",
                    signature.declaration, signature.previous, signature.valid
                );
                compile_fixture(&code, c"ps_5_0")?
            } else {
                read(pixel_path)?
            };
            let mut ps = None;
            unsafe {
                self.device
                    .CreatePixelShader(&pixel, None::<&ID3D11ClassLinkage>, Some(&mut ps))
            }?;
            let vertices = [
                [-0.8, -0.7, 0.5, 1.0],
                [0.0, 0.8, 0.5, 1.0],
                [0.8, -0.7, 0.5, 1.0],
            ]
            .map(|position| Vertex::new(position, if detail { 4 } else { 0 }));
            self.vertices(shader, &vertices, detail)?;
            let ctx = &self.context;
            unsafe {
                ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                ctx.GSSetShader(None::<&ID3D11GeometryShader>, None);
                ctx.PSSetShader(ps.as_ref(), None);
            }
            shader.bind(ctx);
            self.raster_state()?;
            self.pixel_inputs()?;
            let mut targets = Vec::with_capacity(3);
            let mut bindings = Vec::with_capacity(3);
            for format in [
                DXGI_FORMAT_R32G32B32A32_FLOAT,
                DXGI_FORMAT_R32G32B32A32_FLOAT,
                DXGI_FORMAT_R16G16B16A16_FLOAT,
            ] {
                let desc = Device::desc([16, 16], format, D3D11_BIND_RENDER_TARGET);
                let texture = self.texture(&desc, None)?;
                let mut view = None;
                unsafe {
                    self.device
                        .CreateRenderTargetView(&texture, None, Some(&mut view))
                }?;
                let view = view.ok_or("fixture RTV unavailable")?;
                unsafe { ctx.ClearRenderTargetView(&view, &[0.0; 4]) };
                targets.push((texture, desc));
                bindings.push(Some(view));
            }
            unsafe {
                ctx.OMSetRenderTargets(Some(&bindings), None::<&ID3D11DepthStencilView>);
                ctx.Draw(3, 0);
                ctx.OMSetRenderTargets(None, None::<&ID3D11DepthStencilView>);
            }
            // The center texel (8,8) of each target.
            const CENTER: usize = (8 * 16 + 8) * 4;
            let mut result = [[0.0; 4]; 3];
            for (out, (texture, desc)) in result.iter_mut().zip(&targets) {
                // RGBA32F and RGBA16F targets: four components per texel.
                *out = if desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT {
                    let texels = unsafe { self.read::<u16>(texture, desc, 4) }?;
                    std::array::from_fn(|i| half(texels[CENTER + i]))
                } else {
                    let texels = unsafe { self.read::<f32>(texture, desc, 4) }?;
                    std::array::from_fn(|i| texels[CENTER + i])
                };
            }
            Ok(result)
        }
    }

    fn equal(actual: [f32; 4], expected: [f32; 4], label: &str) -> Result<()> {
        for (i, (a, b)) in actual.into_iter().zip(expected).enumerate() {
            require(
                a.is_finite() && (a - b).abs() < 0.00001,
                format!("{label} component {i}: {a} != {b}"),
            )?;
        }
        Ok(())
    }

    fn invalid_detail(gpu: &Gpu, changed: &mut Shader) -> Result<()> {
        let mut state = [0.0; 61 * 4];
        for (prev, message) in [
            ([0.0; 3], "missing stable item history"),
            ([1.0, 0.5, 0.0], "missing previous fade"),
            ([1.0, f32::NAN, 0.75], "nonfinite previous scale"),
        ] {
            state[4..7].copy_from_slice(&prev);
            changed.floats(c"rsf_previous_item_state", &state)?;
            require(gpu.draw(changed, true, true, 4)?.valid == 0.0, message)?;
        }
        history(changed)?;
        let mut bad_rows = rows(true);
        bad_rows[16] = f32::NAN;
        changed.floats(c"rsf_previous_rows", &bad_rows)?;
        require(
            gpu.draw(changed, true, true, 4)?.valid == 0.0,
            "nonfinite previous instance row",
        )?;
        history(changed)?;
        for (row, message) in [
            (2, "unaligned previous row"),
            (244, "out of range previous row"),
        ] {
            require(gpu.draw(changed, true, true, row)?.valid == 0.0, message)?;
        }
        Ok(())
    }

    pub fn run() -> Result<()> {
        let args: Vec<_> = std::env::args_os().collect();
        require(args.len() == 2, "fixture requires DXBC folder")?;
        let folder = Path::new(&args[1]);
        let gpu = Gpu::new()?;
        for tag in [
            "tree",
            "tree_bump",
            "tree_disabled",
            "tree_still",
            "tree_still_bump",
            "detail_wave",
            "detail_still",
        ] {
            let detail = tag.starts_with("detail");
            let original = folder.join(format!("{tag}-original.dxbc"));
            let mut current = Shader::new(&gpu, &original)?;
            let mut previous = Shader::new(&gpu, &original)?;
            let mut changed = Shader::new(&gpu, &folder.join(format!("{tag}-changed.dxbc")))?;
            setup(&mut current, false)?;
            setup(&mut previous, true)?;
            setup(&mut changed, false)?;
            history(&mut changed)?;
            for &row in if detail { &[4, 240][..] } else { &[0][..] } {
                let native = gpu.draw(&current, false, detail, row)?;
                let reference = gpu.draw(&previous, false, detail, row)?;
                let result = gpu.draw(&changed, true, detail, row)?;
                equal(
                    result.current,
                    native.current,
                    &format!("{tag} unchanged current"),
                )?;
                equal(
                    result.previous,
                    reference.current,
                    &format!("{tag} previous deformation reference"),
                )?;
                require(
                    result.valid == 1.0,
                    format!("{tag} valid history unexpectedly rejected"),
                )?;
                let [x, y, _, w] = result.current;
                let [px, py, _, pw] = result.previous;
                let dx = 0.5 * (x / w - px / pw);
                let dy = 0.5 * (py / pw - y / w);
                require(
                    dx.abs() + dy.abs() > 0.0001,
                    "fixture must generate nonzero deformation motion",
                )?;
                println!(
                    "{tag} row={row} current=({x},{y}) previous=({px},{py}) UV_delta=({dx},{dy}) previous-state GPU reference matches"
                );
            }
            let kind = if tag.contains("bump") { "bump" } else { "flat" };
            let pixel_original = folder.join(format!("pixel-{kind}-original.dxbc"));
            let pixel_changed = folder.join(format!("pixel-{kind}-changed.dxbc"));
            let native = gpu.raster(&current, &pixel_original, detail, false)?;
            let targets = gpu.raster(&changed, &pixel_changed, detail, false)?;
            let diagnostic = gpu.raster(&changed, &pixel_changed, detail, true)?;
            equal(targets[0], native[0], &format!("{tag} unchanged MRT0"))?;
            equal(targets[1], native[1], &format!("{tag} unchanged MRT1"))?;
            let [dx, dy, valid, spare] = targets[2];
            require(
                diagnostic[1][0] == 1.0 && valid == 1.0 && spare == 0.0,
                format!("{tag} MRT valid coverage"),
            )?;
            let [px, py, _, pw] = diagnostic[0];
            let x = 8.5 / 16.0 - (px / pw * 0.5 + 0.5);
            let y = 8.5 / 16.0 - (py / pw * -0.5 + 0.5);
            require(
                (dx - x).abs() < 0.00025 && (dy - y).abs() < 0.00025,
                format!("{tag} motion MRT must match interpolated previous clip"),
            )?;
            require(
                dx.abs() + dy.abs() > 0.0001,
                format!("{tag} motion cannot be manufactured zero"),
            )?;
            println!(
                "{tag} MRT2 rendered=({dx},{dy},{valid},{spare}) RGBA16_FLOAT, original MRT0/1 unchanged"
            );
            flags(&mut changed, 0)?;
            require(
                gpu.draw(&changed, true, detail, if detail { 4 } else { 0 })?
                    .valid
                    == 0.0,
                "missing draw history must invalidate",
            )?;
            let invalid = gpu.raster(&changed, &pixel_changed, detail, false)?;
            equal(
                invalid[2],
                [0.0; 4],
                &format!("{tag} missing history clears motion coverage"),
            )?;
            equal(
                invalid[0],
                native[0],
                &format!("{tag} invalid history must preserve native MRT0"),
            )?;
            if detail {
                history(&mut changed)?;
                invalid_detail(&gpu, &mut changed)?;
                println!("{tag} invalid fade/scale and invalid row indices rejected");
            }
            println!("{tag} missing history invalidates");
        }
        Ok(())
    }
}

fn main() {
    #[cfg(windows)]
    if let Err(error) = fixture::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    std::process::exit(77);
}
