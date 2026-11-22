// SPDX-License-Identifier: MIT
use super::*;
use crate::sys::rsf_frame_tap_geometry as Geometry;
use crate::sys::rsf_observer_layout_element;
use crate::velocity_pass_cache::rsf_ac7_velocity_layout_seen;
use crate::velocity_renderer::replay::*;
use rsf_game_sys::{
    RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_TRANSLUCENT_MESH,
    RSF_GAME_RENDER_VELOCITY_FINALIZE, rsf_game_render_pass as Pass,
};
use std::cell::Cell;
use std::ffi::{c_char, c_void};
use std::ptr::{from_ref, null_mut};
use windows::Win32::Graphics::{Direct3D::*, Direct3D11::*, Dxgi::Common::*};
use windows::core::{BOOL, Interface, PCSTR, Result};

unsafe extern "C" {
    fn rsf_ac7_velocity_shader_seen(
        shader: *mut c_void,
        stage: u32,
        bytes: *const c_void,
        size: u32,
    );
}

const VIEWPORT: D3D11_VIEWPORT = D3D11_VIEWPORT {
    TopLeftX: 0.,
    TopLeftY: 0.,
    Width: 2.,
    Height: 1.,
    MinDepth: 0.,
    MaxDepth: 1.,
};
const NEUTRAL: f32 = 32767. / 65535.;
const OPAQUE: [f32; 4] = [0.7, 0.6, 0., 0.];

struct Warp {
    device: ID3D11Device,
    c: ID3D11DeviceContext,
    ok: Cell<bool>,
}

impl Warp {
    fn new() -> Option<Self> {
        let (mut device, mut c) = (None, None);
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                Default::default(),
                Default::default(),
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut c),
            )
        }
        .ok()?;
        Some(Self {
            device: device?,
            c: c?,
            ok: Cell::new(true),
        })
    }

    fn check(&self, value: bool, description: &str) {
        self.ok.set(check(value, description) && self.ok.get());
    }

    fn make<T>(
        &self,
        description: &str,
        create: impl FnOnce(*mut Option<T>) -> Result<()>,
    ) -> Option<T> {
        let mut out = None;
        if let Err(error) = create(&mut out) {
            eprintln!("{description}: {:08x}", error.code().0 as u32);
            self.check(false, description);
            return None;
        }
        self.check(out.is_some(), description);
        out
    }

    fn observe<T: Interface>(&self, shader: &T, stage: u32, code: &[u8]) {
        unsafe {
            rsf_ac7_velocity_shader_seen(
                shader.as_raw(),
                stage,
                code.as_ptr().cast(),
                code.len() as u32,
            )
        };
    }

    fn vertex(&self, code: &[u8], name: &str) -> Option<ID3D11VertexShader> {
        self.make(name, |out| unsafe {
            self.device.CreateVertexShader(code, None, Some(out))
        })
    }

    fn pixel(&self, code: &[u8], name: &str) -> Option<ID3D11PixelShader> {
        self.make(name, |out| unsafe {
            self.device.CreatePixelShader(code, None, Some(out))
        })
    }

    fn buffer(
        &self,
        bytes: u32,
        bind: D3D11_BIND_FLAG,
        misc: u32,
        name: &str,
    ) -> Option<ID3D11Buffer> {
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: bytes,
            BindFlags: bind.0 as u32,
            MiscFlags: misc,
            ..Default::default()
        };
        self.make(name, |out| unsafe {
            self.device.CreateBuffer(&desc, None, Some(out))
        })
    }

    fn initialized_buffer<T>(
        &self,
        value: &T,
        bind: D3D11_BIND_FLAG,
        usage: D3D11_USAGE,
        name: &str,
    ) -> Option<ID3D11Buffer> {
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: size_of::<T>() as u32,
            BindFlags: bind.0 as u32,
            Usage: usage,
            ..Default::default()
        };
        let initial = D3D11_SUBRESOURCE_DATA {
            pSysMem: from_ref(value).cast(),
            SysMemPitch: 0,
            SysMemSlicePitch: 0,
        };
        self.make(name, |out| unsafe {
            self.device.CreateBuffer(&desc, Some(&initial), Some(out))
        })
    }

    fn texture(
        &self,
        format: DXGI_FORMAT,
        bind: D3D11_BIND_FLAG,
        initial: Option<&D3D11_SUBRESOURCE_DATA>,
        name: &str,
    ) -> Option<ID3D11Texture2D> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: 2,
            Height: 1,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BindFlags: bind.0 as u32,
            ..Default::default()
        };
        self.make(name, |out| unsafe {
            self.device
                .CreateTexture2D(&desc, initial.map(from_ref), Some(out))
        })
    }

    fn update<T>(&self, buffer: &ID3D11Buffer, data: &T) {
        unsafe {
            self.c
                .UpdateSubresource(buffer, 0, None, from_ref(data).cast(), 0, 0)
        };
    }

    fn read<T: Copy>(&self, source: &ID3D11Texture2D, create: &str, read: &str) -> Option<T> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { source.GetDesc(&mut desc) };
        desc.BindFlags = 0;
        desc.Usage = D3D11_USAGE_STAGING;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let readable = self.make(create, |out| unsafe {
            self.device.CreateTexture2D(&desc, None, Some(out))
        })?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { self.c.CopyResource(&readable, source) };
        let success = unsafe {
            self.c
                .Map(&readable, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .is_ok();
        if !success {
            self.check(false, read);
            return None;
        }
        let valid = !mapped.pData.is_null() && mapped.RowPitch as usize >= size_of::<T>();
        let value = valid.then(|| unsafe { mapped.pData.cast::<T>().read_unaligned() });
        unsafe { self.c.Unmap(&readable, 0) };
        self.check(valid, read);
        value
    }

    fn stream(&self, buffer: &ID3D11Buffer, stride: u32) {
        unsafe {
            self.c
                .IASetVertexBuffers(0, 1, Some(&Some(buffer.clone())), Some(&stride), Some(&0))
        };
    }

    fn layout(
        &self,
        code: &[u8],
        elements: &[D3D11_INPUT_ELEMENT_DESC],
        name: &str,
    ) -> Option<ID3D11InputLayout> {
        self.make(name, |out| unsafe {
            self.device.CreateInputLayout(elements, code, Some(out))
        })
    }
}

fn element(
    index: u32,
    format: DXGI_FORMAT,
    offset: u32,
    instance: bool,
) -> D3D11_INPUT_ELEMENT_DESC {
    D3D11_INPUT_ELEMENT_DESC {
        SemanticName: PCSTR(c"ATTRIBUTE".as_ptr().cast()),
        SemanticIndex: index,
        Format: format,
        AlignedByteOffset: offset,
        InputSlotClass: if instance {
            D3D11_INPUT_PER_INSTANCE_DATA
        } else {
            D3D11_INPUT_PER_VERTEX_DATA
        },
        InstanceDataStepRate: u32::from(instance),
        ..Default::default()
    }
}

struct Targets {
    _texture: ID3D11Texture2D,
    material: ID3D11ShaderResourceView,
    output: ID3D11Texture2D,
    rtv: ID3D11RenderTargetView,
    depth: ID3D11Texture2D,
    dsv: ID3D11DepthStencilView,
    depth_write: ID3D11DepthStencilState,
    raster: ID3D11RasterizerState,
}

impl Targets {
    fn new(gpu: &Warp) -> Option<Self> {
        let pixels = [0.2f32, 0.4, 0.6, 0.8, 0.2, 0.4, 0.6, 0.1];
        let init = D3D11_SUBRESOURCE_DATA {
            pSysMem: pixels.as_ptr().cast(),
            SysMemPitch: size_of_val(&pixels) as u32,
            SysMemSlicePitch: 0,
        };
        let d = &gpu.device;
        let texture = gpu.texture(
            DXGI_FORMAT_R32G32B32A32_FLOAT,
            D3D11_BIND_SHADER_RESOURCE,
            Some(&init),
            "create material texture",
        )?;
        let material = gpu.make("create material texture view", |out| unsafe {
            d.CreateShaderResourceView(&texture, None, Some(out))
        })?;
        let output = gpu.texture(
            DXGI_FORMAT_R32G32_FLOAT,
            D3D11_BIND_RENDER_TARGET,
            None,
            "create velocity target",
        )?;
        let rtv = gpu.make("create velocity target view", |out| unsafe {
            d.CreateRenderTargetView(&output, None, Some(out))
        })?;
        let depth = gpu.texture(
            DXGI_FORMAT_D32_FLOAT,
            D3D11_BIND_DEPTH_STENCIL,
            None,
            "create translucent depth",
        )?;
        let dsv = gpu.make("create translucent depth view", |out| unsafe {
            d.CreateDepthStencilView(&depth, None, Some(out))
        })?;
        let state = D3D11_DEPTH_STENCIL_DESC {
            DepthEnable: true.into(),
            DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL,
            DepthFunc: D3D11_COMPARISON_GREATER_EQUAL,
            ..Default::default()
        };
        let depth_write = gpu.make("create depth-writing pass state", |out| unsafe {
            d.CreateDepthStencilState(&state, Some(out))
        })?;
        let raster = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            DepthClipEnable: true.into(),
            ..Default::default()
        };
        let raster = gpu.make("create raster state", |out| unsafe {
            d.CreateRasterizerState(&raster, Some(out))
        })?;
        Some(Self {
            _texture: texture,
            material,
            output,
            rtv,
            depth,
            dsv,
            depth_write,
            raster,
        })
    }

    fn opacity(&self, gpu: &Warp, vs: &ID3D11VertexShader, retained: Option<&Path>) -> Option<()> {
        #[repr(C)]
        struct Parameters {
            rect: [f32; 4],
            jitter: [f32; 4],
            cutoff: f32,
            history: u32,
            padding: [f32; 2],
        }
        let params = Parameters {
            rect: [0., 0., 2., 1.],
            jitter: [0.; 4],
            cutoff: 0.333 + 1. / 255.,
            history: 1,
            padding: [0.; 2],
        };
        let cb = gpu.initialized_buffer(
            &params,
            D3D11_BIND_CONSTANT_BUFFER,
            D3D11_USAGE_IMMUTABLE,
            "create pass constants",
        )?;
        let c = &gpu.c;
        for (mode, material) in [MATERIAL_0, MATERIAL_1].into_iter().enumerate() {
            let native = compile(material, c"ps_5_0")?;
            if let Some(path) = retained {
                let _ = fs::write(path.join(format!("fixture-{mode}.dxbc")), &native);
            }
            let Some(code) =
                velocity_pixel::transform(&native, mode as u32 + 2, u32::MAX, u32::MAX)
            else {
                gpu.check(false, "PS validation failed");
                continue;
            };
            let Some(ps) = gpu.pixel(&code, "PS validation failed") else {
                continue;
            };
            unsafe {
                c.ClearRenderTargetView(&self.rtv, &[0.; 4]);
                c.ClearDepthStencilView(&self.dsv, D3D11_CLEAR_DEPTH.0, 0.1, 0);
                c.OMSetRenderTargets(Some(&[Some(self.rtv.clone())]), &self.dsv);
                c.OMSetDepthStencilState(&self.depth_write, 0);
                c.OMSetBlendState(None, None, u32::MAX);
                c.RSSetState(&self.raster);
                c.RSSetViewports(Some(&[VIEWPORT]));
                c.VSSetShader(vs, None);
                c.PSSetShader(&ps, None);
                c.PSSetShaderResources(0, Some(&[Some(self.material.clone())]));
                c.PSSetConstantBuffers(13, Some(&[Some(cb.clone())]));
                c.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                c.Draw(3, 0);
                c.OMSetRenderTargets(None, None);
            }
            if let Some(values) =
                gpu.read::<[f32; 4]>(&self.output, "create readback", "read pass output")
            {
                gpu.check((values[0] - (NEUTRAL - 0.2 * 0.2495)).abs() < 1e-5
                    && (values[1] - NEUTRAL).abs() < 1e-5 && values[2] == 0. && values[3] == 0.,
                    "generated-geometry velocity survives; below-threshold translucent/additive fragment is unwritten");
            }
            if let Some(values) =
                gpu.read::<[f32; 2]>(&self.depth, "create readback", "read pass output")
            {
                gpu.check((values[0] - 0.75).abs() < 1e-6 && (values[1] - 0.1).abs() < 1e-6,
                    "opacity-qualified fragment writes depth; clipped fragment preserves opaque depth");
            }
        }
        Some(())
    }
}

struct Replay<'a> {
    gpu: &'a Warp,
    targets: &'a Targets,
    motion: ID3D11Texture2D,
    depth: ID3D11Texture2D,
    motion_rtv: ID3D11RenderTargetView,
    depth_dsv: ID3D11DepthStencilView,
    arguments: ID3D11Buffer,
    material: ID3D11PixelShader,
    material_code: Vec<u8>,
    pass: Pass,
    draw: Geometry,
}

impl<'a> Replay<'a> {
    fn new(
        gpu: &'a Warp,
        targets: &'a Targets,
        material: ID3D11PixelShader,
        material_code: Vec<u8>,
    ) -> Option<Self> {
        let d = &gpu.device;
        let motion = gpu.texture(
            DXGI_FORMAT_R16G16_UNORM,
            D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE,
            None,
            "create actual engine motion input",
        )?;
        let motion_rtv = gpu.make("create actual engine motion view", |out| unsafe {
            d.CreateRenderTargetView(&motion, None, Some(out))
        })?;
        let depth = gpu.texture(
            DXGI_FORMAT_R32_TYPELESS,
            D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE,
            None,
            "create actual engine depth input",
        )?;
        let dsv = D3D11_DEPTH_STENCIL_VIEW_DESC {
            Format: DXGI_FORMAT_D32_FLOAT,
            ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
            ..Default::default()
        };
        let depth_dsv = gpu.make("create actual engine depth view", |out| unsafe {
            d.CreateDepthStencilView(&depth, Some(&dsv), Some(out))
        })?;
        // The C ABI pass consists of integer, floating point and raw pointer fields.
        let mut pass: Pass = unsafe { std::mem::zeroed() };
        pass.struct_size = size_of::<Pass>() as u32;
        pass.role = RSF_GAME_RENDER_TRANSLUCENT_MESH;
        pass.session_id = 1;
        pass.family_key = 2;
        pass.view_key = 3;
        pass.history_key = 4;
        pass.flags = RSF_GAME_RENDER_PRIMARY;
        pass.primitive_key = 5;
        pass.vertex_factory_key = 6;
        pass.material_key = 7;
        pass.material_blend = 2;
        pass.material_opacity_clip = 0.333;
        pass.camera_valid = 1;
        pass.camera.render_width = 2;
        pass.camera.render_height = 1;
        for i in [0, 5, 10, 15] {
            pass.camera.clip_to_previous_clip[i] = 1.;
        }
        let draw = Geometry {
            context: gpu.c.as_raw(),
            width: 2,
            height: 1,
            samples: 1,
            target: targets.output.as_raw(),
            depth_view: depth_dsv.as_raw(),
            kind: 0,
            count: 3,
            topology: D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST.0 as u32,
            ..Default::default()
        };
        let arguments = gpu.buffer(
            48,
            D3D11_BIND_FLAG(0),
            D3D11_RESOURCE_MISC_DRAWINDIRECT_ARGS.0 as u32,
            "create native GPU indirect arguments",
        )?;
        Some(Self {
            gpu,
            targets,
            motion,
            depth,
            motion_rtv,
            depth_dsv,
            arguments,
            material,
            material_code,
            pass,
            draw,
        })
    }

    fn clear(&self) {
        unsafe {
            self.gpu
                .c
                .ClearDepthStencilView(&self.depth_dsv, D3D11_CLEAR_DEPTH.0, 0.1, 0);
            self.gpu.c.ClearRenderTargetView(&self.motion_rtv, &OPAQUE);
        }
    }

    fn target(&self) {
        unsafe {
            self.gpu
                .c
                .OMSetRenderTargets(Some(&[Some(self.targets.rtv.clone())]), &self.depth_dsv)
        };
    }

    fn material_srv(&self) {
        unsafe {
            self.gpu
                .c
                .PSSetShaderResources(0, Some(&[Some(self.targets.material.clone())]))
        };
    }

    fn scope(&self) {
        unsafe { rsf_ac7_velocity_scope(&self.pass) };
    }
    fn geometry(&self) {
        unsafe { rsf_ac7_velocity_geometry(&self.draw) };
    }

    fn commit(&mut self, description: &str, create: &str, read: &str) -> Option<[u16; 4]> {
        self.pass.role = RSF_GAME_RENDER_VELOCITY_FINALIZE;
        self.pass.motion = self.motion.as_raw();
        self.pass.depth = self.depth.as_raw();
        self.gpu.check(
            unsafe { rsf_ac7_velocity_commit(self.gpu.c.as_raw(), &self.pass) } != 0,
            description,
        );
        unsafe { self.gpu.c.OMSetRenderTargets(None, None) };
        self.gpu.read(&self.motion, create, read)
    }

    fn moving(
        &mut self,
        moving: &ID3D11VertexShader,
        alias: &ID3D11PixelShader,
        object: &ID3D11Buffer,
    ) -> Option<()> {
        let gpu = self.gpu;
        let (c, d) = (&gpu.c, &gpu.device);
        self.draw.vertex_shader = moving.as_raw();
        let index = gpu.initialized_buffer(
            &[0u16, 1, 2],
            D3D11_BIND_INDEX_BUFFER,
            D3D11_USAGE_DEFAULT,
            "create native indexed-indirect geometry",
        )?;
        let gs_code = compile(GENERATED_GS, c"gs_5_0")?;
        let gs: ID3D11GeometryShader = gpu
            .make("create native geometry producer", |out| unsafe {
                d.CreateGeometryShader(&gs_code, None, Some(out))
            })?;
        gpu.observe(&gs, 2, &gs_code);
        let constant = |name| gpu.buffer(16, D3D11_BIND_CONSTANT_BUFFER, 0, name);
        let gs_cb = constant("create native GS deformation constants")?;
        let patch_vs = compile(PATCH_VS, c"vs_5_0")?;
        let patch_hs = compile(PATCH_HS, c"hs_5_0")?;
        let patch_ds = compile(PATCH_DS, c"ds_5_0")?;
        let vs = gpu.vertex(&patch_vs, "create native tessellation VS")?;
        let hs: ID3D11HullShader = gpu
            .make("create native adaptive tessellation HS", |out| unsafe {
                d.CreateHullShader(&patch_hs, None, Some(out))
            })?;
        let ds: ID3D11DomainShader = gpu
            .make("create native adaptive tessellation DS", |out| unsafe {
                d.CreateDomainShader(&patch_ds, None, Some(out))
            })?;
        gpu.observe(&vs, 0, &patch_vs);
        gpu.observe(&hs, 3, &patch_hs);
        gpu.observe(&ds, 4, &patch_ds);
        let patch_hs12 = compile(PATCH_HS12, c"hs_5_0")?;
        let hs12: ID3D11HullShader = gpu
            .make("create 12-point native control phase", |out| unsafe {
                d.CreateHullShader(&patch_hs12, None, Some(out))
            })?;
        gpu.observe(&hs12, 3, &patch_hs12);
        let tess = constant("create native tessellation constants")?;
        let deform = constant("create native domain constants")?;
        for frame in 1..=13u32 {
            self.pass.native_frame = frame.into();
            self.pass.role = RSF_GAME_RENDER_TRANSLUCENT_MESH;
            self.pass.flags = RSF_GAME_RENDER_PRIMARY
                | if matches!(frame, 3 | 7 | 10 | 13) {
                    RSF_GAME_RENDER_RESET
                } else {
                    0
                };
            self.pass.material_blend = if frame == 13 { 5 } else { 2 };
            self.pass.geometry_capacity = 3;
            let mut arguments = [0u32; 12];
            arguments[4] = if frame == 12 { 12 } else { 3 };
            arguments[5] = if frame == 4 { 2 } else { 1 };
            gpu.update(&self.arguments, &arguments);
            (self.draw.kind, self.draw.count) = match frame {
                2 => (5, 0),
                3 | 5..=10 => (0, 3),
                11 | 13 => (0, 12),
                _ => (6, 0),
            };
            self.draw.indirect_buffer = self.arguments.as_raw();
            self.draw.indirect_offset = 16;
            unsafe { c.IASetIndexBuffer(&index, DXGI_FORMAT_R16_UINT, 0) };
            self.scope();
            let x = match frame {
                11.. => (frame - 11) as f32 * 0.1,
                8..=10 => (frame - 8) as f32 * 0.1,
                5..=7 => (frame - 5) as f32 * 0.1,
                _ => (frame - 1) as f32 * 0.2,
            };
            gpu.update(object, &[x, 0., 0., 0.]);
            let generated = if frame >= 5 {
                (frame - 5) as f32 * 0.1
            } else {
                0.
            };
            gpu.update(&gs_cb, &[generated, 0., 0., 0.]);
            let native_gs = (5..8).contains(&frame).then_some(&gs);
            let native_hs = match frame {
                11.. => Some(&hs12),
                8..=10 => Some(&hs),
                _ => None,
            };
            let native_ds = (frame >= 8).then_some(&ds);
            unsafe {
                c.GSSetConstantBuffers(1, Some(&[Some(gs_cb.clone())]));
                c.GSSetShader(native_gs, None);
                c.HSSetShader(native_hs, None);
                c.DSSetShader(native_ds, None);
            }
            gpu.update(&tess, &[if frame == 8 { 2f32 } else { 6. }, 0., 0., 0.]);
            let shift = if frame >= 8 { x } else { 0. };
            gpu.update(&deform, &[shift, 0., 0., 0.]);
            unsafe {
                c.HSSetConstantBuffers(1, Some(&[Some(tess.clone())]));
                c.DSSetConstantBuffers(2, Some(&[Some(deform.clone())]));
            }
            self.clear();
            self.target();
            let native_ps = if frame == 9 { alias } else { &self.material };
            let native_vs = if frame >= 8 { &vs } else { moving };
            unsafe {
                c.OMSetDepthStencilState(&self.targets.depth_write, 0);
                c.OMSetBlendState(None, None, u32::MAX);
                c.VSSetShader(native_vs, None);
                c.PSSetShader(native_ps, None);
                c.VSSetConstantBuffers(0, Some(&[Some(object.clone())]));
            }
            self.draw.vertex_shader = native_vs.as_raw();
            self.material_srv();
            let topology = if frame >= 11 {
                D3D_PRIMITIVE_TOPOLOGY_12_CONTROL_POINT_PATCHLIST
            } else if frame >= 8 {
                D3D_PRIMITIVE_TOPOLOGY_3_CONTROL_POINT_PATCHLIST
            } else {
                D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST
            };
            unsafe {
                c.RSSetState(&self.targets.raster);
                c.RSSetViewports(Some(&[VIEWPORT]));
                c.IASetPrimitiveTopology(topology);
            }
            self.draw.topology = topology.0 as u32;
            let count = shader_creation_count();
            self.geometry();
            if frame == 9 {
                gpu.check(shader_creation_count() == count,
                    "a new material object with identical code reuses VS/PS/HS/DS/SO objects without driver shader creation");
            }
            let (mut out_vs, mut out_ps, mut out_gs, mut out_hs, mut out_ds) =
                (None, None, None, None, None);
            unsafe {
                c.VSGetShader(&mut out_vs, None, None);
                c.PSGetShader(&mut out_ps, None, None);
                c.GSGetShader(&mut out_gs, None, None);
                c.HSGetShader(&mut out_hs, None, None);
                c.DSGetShader(&mut out_ds, None, None);
            }
            gpu.check(
                out_vs.as_ref() == Some(native_vs) && out_ps.as_ref() == Some(native_ps),
                "generated-geometry passes restore native shaders",
            );
            gpu.check(
                out_gs.as_ref() == native_gs,
                "velocity replay restores native geometry stage",
            );
            gpu.check(
                out_hs.as_ref() == native_hs && out_ds.as_ref() == native_ds,
                "velocity replay restores native tessellation stages",
            );
            if let Some(values) = self.commit(
                "commit native translucent velocity/depth producer",
                "create committed motion readback",
                "read committed native motion",
            ) {
                let expected = if frame == 4 {
                    0.7
                } else {
                    NEUTRAL
                        + if matches!(frame, 2 | 6 | 9 | 12) {
                            0.2 * 0.2495
                        } else {
                            0.
                        }
                };
                eprintln!(
                    "frame {frame} motion {:.6},{:.6} opaque {:.6} expected {expected:.6}",
                    cast_unit(values[0]),
                    cast_unit(values[1]),
                    cast_unit(values[2])
                );
                gpu.check((cast_unit(values[0]) - expected).abs() < 5e-5
                    && (cast_unit(values[2]) - if frame == 13 { NEUTRAL } else { 0.7 }).abs() < 3e-5,
                    "direct/indirect motion respects GPU arguments, camera cuts, clipped opacity and the native storage bound");
            }
            rsf_ac7_velocity_frame_end();
        }
        rsf_ac7_velocity_release();
        Some(())
    }

    fn sprites(&mut self) -> Option<()> {
        let gpu = self.gpu;
        let (c, d) = (&gpu.c, &gpu.device);
        self.pass.material_blend = 2;
        unsafe {
            c.HSSetShader(None, None);
            c.DSSetShader(None, None);
            c.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        }
        self.draw.topology = D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST.0 as u32;
        let code = compile(PARTICLE_VS, c"vs_5_0")?;
        let vs = gpu.vertex(&code, "create native sprite VS")?;
        gpu.observe(&vs, 0, &code);
        gpu.observe(&self.material, 1, &self.material_code);
        let layout = gpu.layout(
            &code,
            &[
                element(0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, false),
                element(1, DXGI_FORMAT_R32G32B32A32_FLOAT, 16, false),
                element(2, DXGI_FORMAT_R32G32B32A32_FLOAT, 32, false),
                element(4, DXGI_FORMAT_R32G32_FLOAT, 48, false),
            ],
            "create native sprite declaration",
        )?;
        #[repr(C)]
        #[derive(Default, Clone, Copy)]
        struct Particle {
            centre: [f32; 4],
            old: [f32; 4],
            size: [f32; 4],
            corner: [f32; 2],
        }
        let mut particles = [Particle::default(); 6];
        let stream = gpu.buffer(
            size_of_val(&particles) as u32,
            D3D11_BIND_VERTEX_BUFFER,
            0,
            "create native sprite stream",
        )?;
        self.pass.flags = RSF_GAME_RENDER_PRIMARY;
        self.pass.primitive_key = 8;
        self.pass.material_key = 9;
        self.draw.vertex_shader = vs.as_raw();
        self.draw.count = 6;
        self.draw.input_layout = layout.as_raw();
        self.draw.kind = 0;
        self.draw.indirect_buffer = null_mut();
        self.draw.indirect_offset = 0;
        let stride = size_of::<Particle>() as u32;
        self.draw.vertex_buffers[0] = stream.as_raw();
        self.draw.strides[0] = stride;
        let query = D3D11_QUERY_DESC {
            Query: D3D11_QUERY_OCCLUSION_PREDICATE,
            MiscFlags: 0,
        };
        let predicate: ID3D11Predicate = gpu.make("create empty-draw predicate", |out| unsafe {
            d.CreatePredicate(&query, Some(out))
        })?;
        unsafe {
            c.Begin(&predicate);
            c.End(&predicate);
        }
        for frame in 0..5u32 {
            let corners = [[-1., -1.], [3., -1.], [-1., 3.]];
            for (i, particle) in particles.iter_mut().enumerate() {
                let id = if frame == 0 { i / 3 } else { 1 - i / 3 };
                let z = if id == 0 { 0.75 } else { 0.5 };
                let x = (if id == 0 { 1. } else { -1. }) * frame as f32 * 0.1;
                *particle = Particle {
                    centre: [x, 0., z, 0.],
                    old: [0., 0., z, (id + 1) as f32 / 10000.],
                    size: [1., 1., 0., 0.],
                    corner: corners[i % 3],
                };
            }
            gpu.update(&stream, &particles);
            self.pass.native_frame = u64::from(frame + 8);
            self.pass.role = RSF_GAME_RENDER_TRANSLUCENT_MESH;
            self.pass.vertex_factory_key = if frame == 0 { 6 } else { 900 };
            self.scope();
            self.clear();
            self.target();
            unsafe {
                c.VSSetShader(&vs, None);
                c.PSSetShader(&self.material, None);
                c.GSSetShader(None, None);
                c.IASetInputLayout(&layout);
            }
            gpu.stream(&stream, stride);
            self.material_srv();
            self.draw.kind = if frame == 1 { 6 } else { 0 };
            self.draw.indirect_buffer = self.arguments.as_raw();
            self.pass.geometry_capacity = 4096;
            let mut arguments = [0u32; 12];
            arguments[..2].copy_from_slice(&[3, 1]);
            gpu.update(&self.arguments, &arguments);
            self.scope();
            if frame == 3 {
                unsafe { c.SetPredication(&predicate, false) };
            }
            self.geometry();
            if frame == 3 {
                let mut restored = None;
                let mut value = BOOL(1);
                unsafe { c.GetPredication(Some(&mut restored), Some(&mut value)) };
                gpu.check(
                    restored.as_ref() == Some(&predicate) && !value.as_bool(),
                    "velocity preserves native predication",
                );
                unsafe { c.SetPredication(None, false) };
            }
            if let Some(values) = self.commit(
                "commit sorted sprite motion",
                "create sorted sprite readback",
                "read sorted sprite motion",
            ) {
                let expected = if frame == 3 {
                    0.7
                } else {
                    NEUTRAL - if frame == 1 { 0.1 * 0.2495 } else { 0. }
                };
                gpu.check((cast_unit(values[0]) - expected).abs() < 5e-5,
                    "sorted sprite history excludes an indirect draw's unwritten tail and a predicated-out draw");
            }
            rsf_ac7_velocity_frame_end();
        }
        rsf_ac7_velocity_release();
        Some(())
    }

    fn mesh(&mut self) -> Option<()> {
        let gpu = self.gpu;
        let (c, d) = (&gpu.c, &gpu.device);
        let code = compile(MESH_VS, c"vs_5_0")?;
        let vs = gpu.vertex(&code, "create native mesh-particle shader")?;
        gpu.observe(&vs, 0, &code);
        gpu.observe(&self.material, 1, &self.material_code);
        let layout = gpu.layout(
            &code,
            &[
                element(8, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, true),
                element(9, DXGI_FORMAT_R32G32B32A32_FLOAT, 16, true),
                element(10, DXGI_FORMAT_R32G32B32A32_FLOAT, 32, true),
            ],
            "create sorted mesh-particle declaration",
        )?;
        type Transform = [f32; 12];
        let mut current = [[0f32; 12]; 2];
        let mut previous = current;
        let instance = gpu.buffer(
            size_of_val(&current) as u32,
            D3D11_BIND_VERTEX_BUFFER,
            0,
            "create current instance transforms",
        )?;
        let history = gpu.buffer(
            size_of_val(&current) as u32,
            D3D11_BIND_SHADER_RESOURCE,
            0,
            "create native previous instance transforms",
        )?;
        let srv = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            ViewDimension: D3D_SRV_DIMENSION_BUFFER,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Buffer: D3D11_BUFFER_SRV {
                    Anonymous1: Default::default(),
                    Anonymous2: D3D11_BUFFER_SRV_1 { NumElements: 6 },
                },
            },
        };
        let previous_view = gpu.make("create native previous-transform SRV", |out| unsafe {
            d.CreateShaderResourceView(&history, Some(&srv), Some(out))
        })?;
        self.pass.primitive_key = 50;
        self.pass.material_key = 51;
        self.pass.flags = RSF_GAME_RENDER_PRIMARY;
        self.draw.vertex_shader = vs.as_raw();
        self.draw.input_layout = layout.as_raw();
        self.draw.kind = 2;
        self.draw.count = 3;
        self.draw.instances = 2;
        self.draw.vertex_buffers[0] = instance.as_raw();
        let stride = size_of::<Transform>() as u32;
        self.draw.strides[0] = stride;
        for frame in 0..2u32 {
            for slot in 0..2 {
                let id = if frame == 0 { slot } else { 1 - slot };
                let mut pose = [
                    1.,
                    0.,
                    0.,
                    0.,
                    0.,
                    1.,
                    0.,
                    0.,
                    0.,
                    0.,
                    1.,
                    if id == 0 { 0. } else { -0.25 },
                ];
                previous[slot] = pose;
                pose[3] = (if id == 0 { 1. } else { -1. }) * frame as f32 * 0.2;
                current[slot] = pose;
            }
            gpu.update(&instance, &current);
            gpu.update(&history, &previous);
            self.pass.native_frame = u64::from(20 + frame);
            self.pass.role = RSF_GAME_RENDER_TRANSLUCENT_MESH;
            self.pass.vertex_factory_key = if frame == 0 { 6 } else { 777 };
            self.scope();
            self.clear();
            self.target();
            unsafe {
                c.VSSetShader(&vs, None);
                c.PSSetShader(&self.material, None);
                c.GSSetShader(None, None);
                c.IASetInputLayout(&layout);
            }
            gpu.stream(&instance, stride);
            unsafe { c.VSSetShaderResources(127, Some(&[Some(previous_view.clone())])) };
            self.material_srv();
            self.geometry();
            if let Some(values) = self.commit(
                "commit reordered mesh-particle history",
                "create mesh-particle motion readback",
                "read reordered instance motion",
            ) {
                let expected = NEUTRAL + if frame == 1 { 0.2 * 0.2495 } else { 0. };
                gpu.check((cast_unit(values[0]) - expected).abs() < 5e-5,
                    "mesh-particle motion follows prior transforms despite instance sorting and factory recycling");
            }
            rsf_ac7_velocity_frame_end();
        }
        rsf_ac7_velocity_release();
        Some(())
    }

    fn beams(&mut self) -> Option<()> {
        let gpu = self.gpu;
        let c = &gpu.c;
        let code = compile(BEAM_VS, c"vs_5_0")?;
        let vs = gpu.vertex(&code, "create native beam VS")?;
        gpu.observe(&vs, 0, &code);
        gpu.observe(&self.material, 1, &self.material_code);
        let elements = [
            element(0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, false),
            element(1, DXGI_FORMAT_R32G32B32_FLOAT, 16, false),
            element(2, DXGI_FORMAT_R32G32B32A32_FLOAT, 32, false),
            element(4, DXGI_FORMAT_R32G32B32A32_FLOAT, 48, false),
            element(3, DXGI_FORMAT_R32G32B32A32_FLOAT, 64, false),
        ];
        let facts = elements.map(|entry| rsf_observer_layout_element {
            semantic_index: entry.SemanticIndex,
            format: entry.Format.0 as u32,
            input_slot: 0,
            byte_offset: entry.AlignedByteOffset,
            per_instance: 0,
        });
        let layout = gpu.layout(
            &code,
            &elements,
            "create native float3-old-position beam layout",
        )?;
        unsafe { rsf_ac7_velocity_layout_seen(layout.as_raw(), facts.as_ptr(), 5, 5) };
        let mut vertices = [[0f32; 20]; 10];
        let buffer = gpu.buffer(
            size_of_val(&vertices) as u32,
            D3D11_BIND_VERTEX_BUFFER,
            0,
            "create native 80-byte beam stream",
        )?;
        self.draw.vertex_shader = vs.as_raw();
        self.draw.input_layout = layout.as_raw();
        self.draw.kind = 0;
        self.draw.topology = D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP.0 as u32;
        self.draw.strides[0] = 80;
        self.draw.vertex_buffers[0] = buffer.as_raw();
        self.pass.flags = RSF_GAME_RENDER_PRIMARY;
        self.pass.primitive_key = 40;
        self.pass.material_key = 41;
        self.pass.material_blend = 2;
        unsafe {
            c.HSSetShader(None, None);
            c.DSSetShader(None, None);
            c.GSSetShader(None, None);
            c.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            c.IASetInputLayout(&layout);
        }
        for kind in 0..3u32 {
            for frame in 0..3u32 {
                self.pass.primitive_key = u64::from(40 + kind);
                self.pass.native_frame = u64::from(30 + kind * 10 + frame);
                self.pass.role = RSF_GAME_RENDER_TRANSLUCENT_MESH;
                self.pass.vertex_factory_key = if frame == 0 { 997 } else { 998 };
                self.pass.flags =
                    RSF_GAME_RENDER_PRIMARY | if frame == 2 { RSF_GAME_RENDER_RESET } else { 0 };
                self.scope();
                self.clear();
                for order in 0..2 {
                    let id = if frame == 0 { order } else { 1 - order };
                    let pairs = match (kind, frame) {
                        (0, 0) => 3,
                        (0, _) => 5,
                        (_, 0) => 2,
                        _ => 4,
                    };
                    let counter = 0x00300000 | if id != 0 { 66 } else { 55 };
                    let low = 0x20000000 | counter;
                    for sample in 0..pairs {
                        for side in 0..2 {
                            let v = &mut vertices[(sample * 2 + side) as usize];
                            *v = [0.; 20];
                            v[0] = (if side != 0 { 3. } else { -1. })
                                + (if id != 0 { -0.1 } else { 0.2 }) * frame as f32;
                            v[1] = -1. + 4. * sample as f32 / (pairs - 1) as f32;
                            v[2] = if id != 0 { 0.5 } else { 0.75 };
                            v[3] = f32::from_bits(low);
                            if kind == 0 {
                                v[0] += 0.1
                                    * (1. - (2. * sample as f32 / (pairs - 1) as f32 - 1.).abs());
                            }
                            v[4] = v[0];
                            v[5] = v[1];
                            v[6] = v[2];
                            v[7] = f32::from_bits(low);
                            let interval = 0x20000000 | sample | ((pairs - 1) << 12);
                            v[8] = 1.;
                            v[9] = 1.;
                            v[11] = f32::from_bits(interval);
                            v[12..16].fill(1.);
                            v[17] = side as f32;
                            if kind != 0 {
                                let (height, first, second, fraction, steps) = match (frame, sample)
                                {
                                    (0, 0) => (3., counter, counter, 0, 1),
                                    (0, _) => (-1., counter + 1, counter, 0, 1),
                                    (_, 0) => (4., counter - 1, counter - 1, 0, 1),
                                    (_, 1) => (3., counter, counter - 1, 0, 1),
                                    (_, 2) => (1., counter + 1, counter, 1, 2),
                                    _ => (-1., counter + 1, counter, 0, 2),
                                };
                                let encoded = |word| {
                                    f32::from_bits(
                                        0x20000000u32
                                            | (word & 0x1fffffff)
                                            | ((word & 0x20000000) << 2),
                                    )
                                };
                                v[1] = height;
                                v[3] = encoded(first | (kind << 28));
                                v[5] = v[1];
                                v[7] = encoded(second);
                                v[11] = encoded(fraction | (steps << 12));
                            }
                        }
                    }
                    gpu.update(&buffer, &vertices);
                    self.draw.count = pairs * 2;
                    gpu.stream(&buffer, 80);
                    unsafe {
                        c.VSSetShader(&vs, None);
                        c.PSSetShader(&self.material, None);
                    }
                    self.material_srv();
                    self.target();
                    if kind == 0 && frame == 0 && order == 0 {
                        // First grow the shared SO allocation with an empty bounded indirect draw.
                        let original_draw = self.draw;
                        let (primitive, capacity) =
                            (self.pass.primitive_key, self.pass.geometry_capacity);
                        gpu.update(&self.arguments, &[0u32; 12]);
                        self.pass.primitive_key = 999;
                        self.pass.geometry_capacity = 4096;
                        self.scope();
                        self.draw.kind = 6;
                        self.draw.indirect_buffer = self.arguments.as_raw();
                        self.draw.indirect_offset = 0;
                        self.geometry();
                        self.draw = original_draw;
                        self.pass.primitive_key = primitive;
                        self.pass.geometry_capacity = capacity;
                        self.scope();
                    }
                    self.draw.kind = if frame == 1 { 6 } else { 0 };
                    self.draw.indirect_buffer = self.arguments.as_raw();
                    self.draw.indirect_offset = 0;
                    self.pass.geometry_capacity = 4096;
                    self.scope();
                    let mut arguments = [0u32; 12];
                    arguments[..2].copy_from_slice(&[pairs * 2, 1]);
                    gpu.update(&self.arguments, &arguments);
                    self.geometry();
                    let restored = unsafe { c.IAGetInputLayout() }.ok();
                    gpu.check(
                        restored.as_ref() == Some(&layout),
                        "beam identity layout remains private to velocity replay",
                    );
                }
                if let Some(values) = self.commit(
                    "commit subdivided/reordered beam motion",
                    "create beam motion readback",
                    "read subdivided beam motion",
                ) {
                    let expected = NEUTRAL + if frame == 1 { 0.2 * 0.2495 } else { 0. };
                    gpu.check((cast_unit(values[0]) - expected).abs() < 5e-5
                        && (cast_unit(values[2]) - 0.7).abs() < 3e-5,
                        "beam/ribbon/animated-trail motion survives subdivision, new heads, draw reorder, factory reuse and cuts");
                }
                rsf_ac7_velocity_frame_end();
            }
        }
        rsf_ac7_velocity_release();
        Some(())
    }
}

fn cast_unit(value: u16) -> f32 {
    f32::from(value) / 65535.
}

unsafe extern "C" fn log(_: *mut c_void, line: *const c_char) {
    eprintln!("{}", unsafe { CStr::from_ptr(line) }.to_string_lossy());
}

struct Cleanup;
impl Drop for Cleanup {
    fn drop(&mut self) {
        rsf_ac7_velocity_release();
        unsafe { rsf_ac7_velocity_set_log(None, null_mut()) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_velocity_warp_fixture(path: *const u16) -> i32 {
    let Some(gpu) = Warp::new() else {
        return 77;
    };
    let path = unsafe { directory(path) };
    let _cleanup = Cleanup;
    gpu.check(
        run(&gpu, path.as_deref()).is_some(),
        "AC7 WARP fixture setup completed",
    );
    i32::from(!gpu.ok.get())
}

fn run(gpu: &Warp, path: Option<&Path>) -> Option<()> {
    let code = compile(FIXTURE_VS, c"vs_5_0")?;
    let vs = gpu.vertex(&code, "create VS")?;
    gpu.check(
        geometry(&gpu.device) == Some(true),
        "validate velocity geometry linkage",
    );
    gpu.check(
        tessellation_fixture(&gpu.device, path) == Some(true),
        "validate velocity tessellation linkage",
    );
    let targets = Targets::new(gpu)?;
    targets.opacity(gpu, &vs, path)?;
    unsafe {
        gpu.c.ClearState();
        rsf_ac7_velocity_set_log(Some(log), null_mut());
    }
    let moving = compile(MOVING_VS, c"vs_5_0")?;
    let material = compile(MATERIAL_0, c"ps_5_0")?;
    let moving_vs = gpu.vertex(&moving, "create native moving VS")?;
    let material_ps = gpu.pixel(&material, "create native material PS")?;
    gpu.observe(&moving_vs, 0, &moving);
    gpu.observe(&material_ps, 1, &material);
    let alias = gpu.pixel(&material, "create identical native material shader object")?;
    gpu.observe(&alias, 1, &material);
    let object = gpu.buffer(
        16,
        D3D11_BIND_CONSTANT_BUFFER,
        0,
        "create native object constants",
    )?;
    let mut replay = Replay::new(gpu, &targets, material_ps, material)?;
    replay.moving(&moving_vs, &alias, &object)?;
    replay.sprites()?;
    replay.mesh()?;
    replay.beams()?;
    gpu.check(
        path.is_none_or(|path| replay_retained(&gpu.device, path) == Some(true)),
        "validate retained DXBC",
    );
    gpu.check(
        history_compile_count() == 1,
        "native/material interface changes link DXBC without repeating history-GS HLSL compilation",
    );
    Some(())
}
