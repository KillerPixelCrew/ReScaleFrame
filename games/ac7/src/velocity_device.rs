// SPDX-License-Identifier: MIT
//! Device-side shader and variant ownership for translucent velocity replay.
use crate::velocity_commit::{FULLSCREEN_SOURCE, MERGE_SOURCE};
use crate::velocity_geometry::GeometryRegisters;
use crate::velocity_pass_cache::{Built, VariantRequest, VariantSources, admit_variant};
use crate::velocity_stage::{HISTORY_WRITER, compile_source};
use crate::velocity_stream::INDIRECT_SOURCE;
use crate::velocity_tessellation::DomainRegisters;
use crate::{velocity_geometry, velocity_pixel, velocity_tessellation, velocity_vertex};
use std::collections::HashMap;
use std::ffi::CStr;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_INPUT_ELEMENT_DESC, D3D11_INPUT_PER_VERTEX_DATA, D3D11_SO_DECLARATION_ENTRY,
    D3D11_SO_NO_RASTERIZED_STREAM, ID3D11ComputeShader, ID3D11Device, ID3D11DomainShader,
    ID3D11GeometryShader, ID3D11HullShader, ID3D11InputLayout, ID3D11PixelShader,
    ID3D11VertexShader,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32G32B32A32_FLOAT;
use windows::core::{PCSTR, Result};

fn create<T>(make: impl FnOnce(*mut Option<T>) -> Result<()>) -> Option<T> {
    let mut shader = None;
    make(&mut shader).ok()?;
    shader
}

fn cached<T: Clone>(
    cache: &mut HashMap<Vec<u8>, T>,
    bytes: &[u8],
    count: &mut u32,
    make: impl FnOnce(*mut Option<T>) -> Result<()>,
) -> Option<T> {
    if let Some(shader) = cache.get(bytes) {
        return Some(shader.clone());
    }
    let shader = create(make)?;
    cache.try_reserve(1).ok()?;
    let mut key = Vec::new();
    key.try_reserve_exact(bytes.len()).ok()?;
    key.extend_from_slice(bytes);
    cache.insert(key, shader.clone());
    *count = count.wrapping_add(1);
    Some(shader)
}

#[derive(Clone, Default)]
pub(super) struct Variant {
    pub(super) vertex: Option<ID3D11VertexShader>,
    pub(super) pixel: Option<ID3D11PixelShader>,
    pub(super) capture: Option<ID3D11GeometryShader>,
    pub(super) geometry: Option<ID3D11GeometryShader>,
    pub(super) hull: Option<ID3D11HullShader>,
    pub(super) corners: Option<ID3D11HullShader>,
    pub(super) domain: Option<ID3D11DomainShader>,
    pub(super) layout: Option<ID3D11InputLayout>,
    pub(super) maximum_vertices: u32,
    pub(super) history_slot: u32,
    pub(super) vertex_hash: u32,
    pub(super) geometry_hash: u32,
    pub(super) input_points: u32,
    pub(super) particle: bool,
}

pub(super) struct Kernels {
    pub(super) hash: ID3D11ComputeShader,
    pub(super) merge: ID3D11PixelShader,
    pub(super) fullscreen: ID3D11VertexShader,
    pub(super) validate_indirect: ID3D11ComputeShader,
}

impl Kernels {
    fn new(device: &ID3D11Device) -> Option<Self> {
        let code = compile_source(HISTORY_WRITER, c"ac7-geometry-history", c"cs_5_0")?;
        let hash = create(|out| unsafe { device.CreateComputeShader(&code, None, Some(out)) })?;
        let code = compile_source(MERGE_SOURCE, c"ac7-velocity-merge", c"ps_5_0")?;
        let merge = create(|out| unsafe { device.CreatePixelShader(&code, None, Some(out)) })?;
        let code = compile_source(FULLSCREEN_SOURCE, c"ac7-velocity-merge", c"vs_5_0")?;
        let fullscreen =
            create(|out| unsafe { device.CreateVertexShader(&code, None, Some(out)) })?;
        let code = compile_source(INDIRECT_SOURCE, c"ac7-indirect-geometry-bounds", c"cs_5_0")?;
        let validate_indirect =
            create(|out| unsafe { device.CreateComputeShader(&code, None, Some(out)) })?;
        Some(Self {
            hash,
            merge,
            fullscreen,
            validate_indirect,
        })
    }
}

pub(super) struct Device {
    device: ID3D11Device,
    pub(super) kernels: Kernels,
    variants: Vec<Variant>,
    vertex: HashMap<Vec<u8>, ID3D11VertexShader>,
    pixel: HashMap<Vec<u8>, ID3D11PixelShader>,
    hull: HashMap<Vec<u8>, ID3D11HullShader>,
    domain: HashMap<Vec<u8>, ID3D11DomainShader>,
    geometry: HashMap<Vec<u8>, ID3D11GeometryShader>,
    capture: HashMap<Vec<u8>, ID3D11GeometryShader>,
    pub(super) count: u32,
}

enum Failure {
    Refused,
    Retry(Option<&'static CStr>),
}

impl Device {
    fn build(
        &mut self,
        sources: &VariantSources<'_>,
        request: &VariantRequest,
    ) -> std::result::Result<u32, Failure> {
        use Failure::{Refused, Retry};
        const TRANSFORM: Failure =
            Retry(Some(c"native material or vertex shader transform refused"));
        const VARIANT: Failure = Retry(Some(c"D3D11 variant creation failed"));
        let [vertex, pixel, geometry, hull, domain] = sources.bytes;
        let native_geometry = !request.shaders[2].is_null();
        let native_hull = !request.shaders[3].is_null();
        let (vertex, particle, mut outputs) =
            velocity_vertex::transform(vertex, native_hull, sources.beam_layout)
                .ok_or(TRANSFORM)?;
        let mut made = Variant {
            maximum_vertices: 1,
            vertex_hash: sources.vertex_hash,
            particle: particle != 0,
            ..Variant::default()
        };
        let device = &self.device;
        if particle == 3 {
            if native_geometry || native_hull {
                return Err(Refused);
            }
            let mut elements = Vec::new();
            elements
                .try_reserve_exact(sources.elements.len())
                .map_err(|_| VARIANT)?;
            for e in sources.elements {
                elements.push(D3D11_INPUT_ELEMENT_DESC {
                    SemanticName: PCSTR(c"ATTRIBUTE".as_ptr().cast()),
                    SemanticIndex: e.semantic_index,
                    Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                    InputSlot: e.input_slot,
                    AlignedByteOffset: e.byte_offset,
                    InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                    InstanceDataStepRate: 0,
                });
            }
            made.layout =
                create(|out| unsafe { device.CreateInputLayout(&elements, &vertex, Some(out)) });
            if made.layout.is_none() {
                return Err(Retry(Some(c"D3D11 beam layout creation failed")));
            }
        }
        let mut registers = GeometryRegisters::default();
        let geometry = if native_hull {
            let mut domain_registers = DomainRegisters::default();
            let ([normal, corners, domain], semantics) = velocity_tessellation::transform(
                hull,
                domain,
                &outputs,
                &mut made.input_points,
                &mut domain_registers,
            )
            .ok_or(TRANSFORM)?;
            outputs = semantics;
            registers.previous = domain_registers.previous;
            registers.current = domain_registers.current;
            // Attempt all three creations before testing the result, as in the native pass.
            made.hull = cached(&mut self.hull, &normal, &mut self.count, |out| unsafe {
                device.CreateHullShader(&normal, None, Some(out))
            });
            made.corners = cached(&mut self.hull, &corners, &mut self.count, |out| unsafe {
                device.CreateHullShader(&corners, None, Some(out))
            });
            made.domain = cached(&mut self.domain, &domain, &mut self.count, |out| unsafe {
                device.CreateDomainShader(&domain, None, Some(out))
            });
            if made.hull.is_none() || made.corners.is_none() || made.domain.is_none() {
                return Err(Retry(Some(c"D3D11 tessellation variant creation failed")));
            }
            made.maximum_vertices = 3u32.div_ceil(made.input_points);
            domain
        } else if native_geometry {
            let (code, semantics) =
                velocity_geometry::native(geometry, &outputs, &mut registers).ok_or(TRANSFORM)?;
            outputs = semantics;
            made.maximum_vertices = registers.maximum_vertices;
            code
        } else {
            velocity_geometry::history(&outputs, &mut registers).ok_or(Retry(None))?
        };
        if native_geometry || native_hull {
            made.history_slot = 127;
            made.geometry_hash = sources.geometry_hash;
        }
        let pixel = velocity_pixel::transform(
            pixel,
            request.material_blend,
            registers.previous,
            registers.current,
        )
        .ok_or(TRANSFORM)?;
        made.vertex = cached(&mut self.vertex, &vertex, &mut self.count, |out| unsafe {
            device.CreateVertexShader(&vertex, None, Some(out))
        });
        if !native_hull {
            made.geometry = cached(
                &mut self.geometry,
                &geometry,
                &mut self.count,
                |out| unsafe { device.CreateGeometryShader(&geometry, None, Some(out)) },
            );
        }
        made.pixel = cached(&mut self.pixel, &pixel, &mut self.count, |out| unsafe {
            device.CreatePixelShader(&pixel, None, Some(out))
        });
        if made.vertex.is_none()
            || (!native_hull && made.geometry.is_none())
            || made.pixel.is_none()
        {
            return Err(VARIANT);
        }
        let position = outputs.iter().find(|s| s.system == 1).ok_or(Refused)?;
        let mut name = Vec::new();
        name.try_reserve_exact(position.name.len() + 1)
            .map_err(|_| VARIANT)?;
        name.extend_from_slice(position.name);
        name.push(0);
        let name = CStr::from_bytes_with_nul(&name).map_err(|_| Refused)?;
        let declarations = [
            (name, position.index),
            (c"RSFKEY", 0),
            (c"RSFCENTRE", 0),
            (c"RSFOLD", 0),
        ]
        .map(|(name, index)| D3D11_SO_DECLARATION_ENTRY {
            Stream: 0,
            SemanticName: PCSTR(name.as_ptr().cast()),
            SemanticIndex: index,
            StartComponent: 0,
            ComponentCount: 4,
            OutputSlot: 0,
        });
        let capture = if native_geometry || native_hull {
            &geometry
        } else {
            &vertex
        };
        made.capture = cached(&mut self.capture, capture, &mut self.count, |out| unsafe {
            device.CreateGeometryShaderWithStreamOutput(
                capture,
                Some(&declarations),
                Some(&[64]),
                D3D11_SO_NO_RASTERIZED_STREAM,
                None,
                Some(out),
            )
        });
        if made.capture.is_none() {
            return Err(Retry(Some(c"stream-output signature creation failed")));
        }
        self.variants.try_reserve(1).map_err(|_| VARIANT)?;
        let slot = self.variants.len() as u32;
        self.variants.push(made);
        Ok(slot)
    }
}

impl Device {
    pub(super) fn new(device: &ID3D11Device) -> Option<Self> {
        Some(Self {
            device: device.clone(),
            kernels: Kernels::new(device)?,
            variants: Vec::new(),
            vertex: HashMap::new(),
            pixel: HashMap::new(),
            hull: HashMap::new(),
            domain: HashMap::new(),
            geometry: HashMap::new(),
            capture: HashMap::new(),
            count: 0,
        })
    }

    pub(super) fn variant(&mut self, request: &VariantRequest) -> Option<Variant> {
        let slot = admit_variant(request, |sources| match self.build(sources, request) {
            Ok(index) => Built::Slot(index),
            Err(Failure::Refused) => Built::Refused,
            Err(Failure::Retry(reason)) => {
                if let Some(reason) = reason {
                    crate::velocity_commit::refused(reason);
                }
                Built::Retry
            }
        });
        self.variants.get(slot as usize).cloned()
    }
}
