// SPDX-License-Identifier: MIT
//! AC7 shader linkage, retained-bytecode and WARP replay fixtures.
#[path = "ac7_velocity_warp.rs"]
mod warp;
use crate::velocity_geometry::{GeometryRegisters, history_compile_count};
use crate::velocity_stage::compile_source;
use crate::velocity_tessellation::DomainRegisters;
use crate::{velocity_geometry, velocity_pixel, velocity_tessellation, velocity_vertex};
use std::ffi::{CStr, OsString};
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use windows::Win32::Graphics::Direct3D11::ID3D11Device;

fn check(value: bool, description: &str) -> bool {
    if !value {
        eprintln!("{description}");
    }
    value
}

fn compile(source: &str, profile: &CStr) -> Option<Vec<u8>> {
    compile_source(source, c"velocity-fixture", profile)
}

fn geometry(device: &ID3D11Device) -> Option<bool> {
    let original = compile(VERTEX, c"vs_5_0")?;
    let (keyed, _, semantics) = velocity_vertex::transform(&original, false, false)?;
    let mut vs = None;
    let mut ok = check(
        unsafe { device.CreateVertexShader(&keyed, None, Some(&mut vs)) }.is_ok(),
        "create keyed geometry VS",
    );
    let mut registers = GeometryRegisters::default();
    let code = velocity_geometry::history(&semantics, &mut registers)?;
    let mut gs = None;
    ok &= check(
        unsafe { device.CreateGeometryShader(&code, None, Some(&mut gs)) }.is_ok(),
        "create previous-generated-geometry GS",
    );
    let native = compile(GEOMETRY, c"gs_5_0")?;
    let (code, _) = velocity_geometry::native(&native, &semantics, &mut registers)?;
    ok &= check(
        registers.maximum_vertices == 3,
        "preserve native GS emission count",
    );
    drop(gs);
    let mut gs = None;
    ok &= check(
        unsafe { device.CreateGeometryShader(&code, None, Some(&mut gs)) }.is_ok(),
        "create augmented native GS with its original raw SRV at t126",
    );
    Some(ok)
}

fn tessellation(
    device: &ID3D11Device,
    vertex: &[u8],
    hull: &[u8],
    domain: &[u8],
    points: u32,
) -> Option<(bool, [Vec<u8>; 3])> {
    let (vertex, _, semantics) = velocity_vertex::transform(vertex, true, false)?;
    let mut input_points = 0;
    let ([hull, corners, domain], _) = velocity_tessellation::transform(
        hull,
        domain,
        &semantics,
        &mut input_points,
        &mut DomainRegisters::default(),
    )?;
    let mut vs = None;
    let mut hs = None;
    let mut corner_hs = None;
    let mut ds = None;
    let mut ok = check(
        input_points == points,
        "preserve native hull input patch size",
    );
    ok &= check(
        unsafe { device.CreateVertexShader(&vertex, None, Some(&mut vs)) }.is_ok(),
        "create tessellation keyed VS",
    );
    ok &= check(
        unsafe { device.CreateHullShader(&hull, None, Some(&mut hs)) }.is_ok()
            && unsafe { device.CreateHullShader(&corners, None, Some(&mut corner_hs)) }.is_ok(),
        "create normal/corner native HS variants",
    );
    ok &= check(
        unsafe { device.CreateDomainShader(&domain, None, Some(&mut ds)) }.is_ok(),
        "create native domain motion shader",
    );
    Some((ok, [vertex, hull, domain]))
}

fn tessellation_fixture(device: &ID3D11Device, path: Option<&Path>) -> Option<bool> {
    let vertex = compile(TESSELLATION_VERTEX, c"vs_5_0")?;
    let hull = compile(HULL, c"hs_5_0")?;
    let domain = compile(DOMAIN, c"ds_5_0")?;
    let (ok, codes) = tessellation(device, &vertex, &hull, &domain, 3)?;
    let Some(path) = path else {
        return Some(ok);
    };
    for (name, code) in ["tess-v.dxbc", "tess-h.dxbc", "tess-d.dxbc"]
        .into_iter()
        .zip(codes)
    {
        // The native fixture also continued when an artifact could not be written.
        let _ = fs::write(path.join(name), code);
    }
    let hull = path.join("native-tess-h.dxbc");
    let domain = path.join("native-tess-d.dxbc");
    if !hull.exists() || !domain.exists() {
        return Some(ok);
    }
    let matched = compile(MATCHED_VERTEX, c"vs_5_0")?;
    let vertex = path.join("native-tess-v.dxbc");
    let vertex = if vertex.exists() {
        fs::read(vertex).ok()?
    } else {
        matched
    };
    let (retained, _) = tessellation(
        device,
        &vertex,
        &fs::read(hull).ok()?,
        &fs::read(domain).ok()?,
        12,
    )?;
    Some(ok && retained)
}

fn retained_shader(device: &ID3D11Device, native: &[u8], vertex: bool) -> Option<bool> {
    if vertex {
        let (code, _, semantics) = velocity_vertex::transform(native, false, false)?;
        let mut vs = None;
        let vs_ok = unsafe { device.CreateVertexShader(&code, None, Some(&mut vs)) }.is_ok();
        let code = velocity_geometry::history(&semantics, &mut GeometryRegisters::default())?;
        let mut gs = None;
        let gs_ok = unsafe { device.CreateGeometryShader(&code, None, Some(&mut gs)) }.is_ok();
        Some(vs_ok && gs_ok)
    } else {
        let code = velocity_pixel::transform(native, 2, u32::MAX, u32::MAX)?;
        let mut ps = None;
        Some(unsafe { device.CreatePixelShader(&code, None, Some(&mut ps)) }.is_ok())
    }
}

fn replay_retained(device: &ID3D11Device, directory: &Path) -> Option<bool> {
    let mut ok = true;
    for entry in fs::read_dir(directory).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let vertex = name.starts_with("0-");
        if (!vertex && !name.starts_with("1-")) || !name.ends_with(".dxbc") {
            continue;
        }
        let result = fs::read(entry.path())
            .ok()
            .and_then(|code| retained_shader(device, &code, vertex));
        if result != Some(true) {
            eprintln!("retained shader refused: {name}");
            ok = false;
        }
    }
    Some(ok)
}

// The fixture driver lends a null-terminated Windows path. Empty means no retained replay.
unsafe fn directory(path: *const u16) -> Option<PathBuf> {
    if path.is_null() {
        return None;
    }
    let len = (0..).find(|&i| unsafe { *path.add(i) == 0 })?;
    (len != 0).then(|| {
        PathBuf::from(OsString::from_wide(unsafe {
            std::slice::from_raw_parts(path, len)
        }))
    })
}

const VERTEX: &str =
    r#"float4 main(uint id:SV_VertexID):SV_Position{return float4(id==2?3:-1,id==1?3:-1,.75,1);}"#;

const GEOMETRY: &str = r#"struct V{float4 p:SV_Position;};ByteAddressBuffer native:register(t126);[maxvertexcount(3)]void main(triangle V v[3],inout TriangleStream<V> s){[unroll]for(uint i=0;i<3;i++){V o;o.p=v[i].p+float4(asfloat(native.Load(4*i)),0,0,0);s.Append(o);}}"#;

const TESSELLATION_VERTEX: &str = r#"struct V{float4 p:VS_To_DS_Position;};V main(uint i:SV_VertexID){V o;o.p=float4(i==2?3:-1,i==1?3:-1,.75,1);return o;}"#;

const HULL: &str = r#"struct V{float4 p:VS_To_DS_Position;};struct C{float e[3]:SV_TessFactor;float i:SV_InsideTessFactor;};C constants(InputPatch<V,3> p){C o;o.e[0]=o.e[1]=o.e[2]=4;o.i=4;return o;}[domain("tri")][partitioning("integer")][outputtopology("triangle_cw")][outputcontrolpoints(3)][patchconstantfunc("constants")]V main(InputPatch<V,3> p,uint i:SV_OutputControlPointID){return p[i];}"#;

const DOMAIN: &str = r#"struct V{float4 p:VS_To_DS_Position;};struct C{float e[3]:SV_TessFactor;float i:SV_InsideTessFactor;};[domain("tri")]float4 main(C c,const OutputPatch<V,3> p,float3 d:SV_DomainLocation):SV_Position{return p[0].p*d.x+p[1].p*d.y+p[2].p*d.z;}"#;

const MATCHED_VERTEX: &str = r#"struct V{float4 a:TEXCOORD10;float4 b:TEXCOORD11;float4 c:TEXCOORD7;float3 n:TEXCOORD9;float4 p:VS_To_DS_Position;uint id:VS_To_DS_VertexID;};V main(uint i:SV_VertexID){V o;o.a=o.b=o.c=0;o.n=float3(0,0,1);o.p=float4(i,0,0,1);o.id=i;return o;}"#;

const FIXTURE_VS: &str = r#"
struct V {float4 position:SV_Position;float4 previous:RSFPREV0;float4 current:RSFCUR0;};
V main(uint id:SV_VertexID) {V o;o.position=float4(id==2?3:-1,id==1?3:-1,.75,1);
o.previous=o.position+float4(.2,0,0,0);o.current=o.position;return o;}"#;

const MOVING_VS: &str = r#"cbuffer Object:register(b0){float4 offset;}float4 main(uint id:SV_VertexID):SV_Position{return float4(id==2?3:-1,id==1?3:-1,.75,1)+offset;}"#;

const GENERATED_GS: &str = r#"cbuffer Generated:register(b1){float4 shift;}struct V{float4 p:SV_Position;};[maxvertexcount(3)]void main(triangle V input[3],inout TriangleStream<V> stream){[unroll]for(uint i=0;i<3;i++){V output;output.p=input[i].p+shift;stream.Append(output);}}"#;

const PATCH_VS: &str = r#"cbuffer Object:register(b0){float4 offset;}struct V{float4 p:VS_To_DS_Position;};V main(uint i:SV_VertexID){V o;o.p=float4(i==2?3:-1,i==1?3:-1,.75,1)+offset;return o;}"#;

const PATCH_HS: &str = r#"cbuffer Tess:register(b1){float4 factor;}struct V{float4 p:VS_To_DS_Position;};struct C{float e[3]:SV_TessFactor;float i:SV_InsideTessFactor;};C constants(InputPatch<V,3> p){C o;o.e[0]=o.e[1]=o.e[2]=factor.x;o.i=factor.x;return o;}[domain("tri")][partitioning("fractional_even")][outputtopology("triangle_cw")][outputcontrolpoints(3)][patchconstantfunc("constants")]V main(InputPatch<V,3> p,uint i:SV_OutputControlPointID){return p[i];}"#;

const PATCH_DS: &str = r#"cbuffer Deform:register(b2){float4 shift;}struct V{float4 p:VS_To_DS_Position;};struct C{float e[3]:SV_TessFactor;float i:SV_InsideTessFactor;};[domain("tri")]float4 main(C c,const OutputPatch<V,3> p,float3 d:SV_DomainLocation):SV_Position{float4 o=p[0].p*d.x+p[1].p*d.y+p[2].p*d.z+shift;o.z+=.1*d.x*d.y;return o;}"#;

const PATCH_HS12: &str = r#"cbuffer Tess:register(b1){float4 factor;}struct V{float4 p:VS_To_DS_Position;};struct C{float e[3]:SV_TessFactor;float i:SV_InsideTessFactor;};C constants(InputPatch<V,12> p){C o;o.e[0]=o.e[1]=o.e[2]=factor.x;o.i=factor.x;return o;}[domain("tri")][partitioning("fractional_even")][outputtopology("triangle_cw")][outputcontrolpoints(3)][patchconstantfunc("constants")]V main(InputPatch<V,12> p,uint i:SV_OutputControlPointID){V o;o.p=p[i].p;o.p.z+=.001;return o;}"#;

const PARTICLE_VS: &str = r#"float4 main(float4 centre:ATTRIBUTE0,float4 old:ATTRIBUTE1,float4 size:ATTRIBUTE2,float2 corner:ATTRIBUTE4):SV_Position{return float4(centre.xy+corner,centre.z,1);}"#;

const MESH_VS: &str = r#"float4 main(uint vertex:SV_VertexID,float4 a:ATTRIBUTE8,float4 b:ATTRIBUTE9,float4 c:ATTRIBUTE10):SV_Position {float4 p=float4(vertex==2?3:-1,vertex==1?3:-1,.75,1);return float4(dot(p,a),dot(p,b),dot(p,c),1);}"#;

const BEAM_VS: &str = r#"float4 main(float4 p:ATTRIBUTE0,float3 old:ATTRIBUTE1,float4 size:ATTRIBUTE2,float4 colour:ATTRIBUTE4,float4 uv:ATTRIBUTE3):SV_Position{return float4(p.xyz,1);}"#;

const MATERIAL_0: &str = r#"Texture2D<float4> data:register(t0);float4 main(float4 p:SV_Position):SV_Target0{float4 c=data.Load(int3(int2(p.xy),0));return float4(max(c.xyz,0),saturate(c.w));}"#;

const MATERIAL_1: &str = r#"Texture2D<float4> data:register(t0);float4 main(float4 p:SV_Position):SV_Target0{float4 c=data.Load(int3(int2(p.xy),0));float a=saturate(c.w);return float4(max(c.xyz,0)*a,0);}"#;
