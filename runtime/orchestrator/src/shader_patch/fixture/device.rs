// SPDX-License-Identifier: MIT
//! Synthetic pack, compiler, checksum, and WARP rendering checks.

use super::*;
use std::ffi::OsString;
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

fn win<T>(result: windows::core::Result<T>) -> Test<T> {
    result.map_err(|error| error.to_string())
}

fn pack(rules: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend(1u32.to_le_bytes());
    payload.extend(12u32.to_le_bytes());
    payload.extend(7u64.to_le_bytes());
    payload.extend(b"metadata.txtfixture");
    let parts: [&[u8]; 3] = [br#"{"id":"synthetic"}"#, rules.as_bytes(), &payload];
    let mut bytes = b"RSFPATCH".to_vec();
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(3u32.to_le_bytes());
    bytes.extend(24u64.to_le_bytes());
    let mut offset = 96u64;
    for (kind, section) in (1u32..).zip(parts) {
        bytes.extend(kind.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(offset.to_le_bytes());
        bytes.extend((section.len() as u64).to_le_bytes());
        offset += section.len() as u64;
    }
    for part in parts {
        bytes.extend(part);
    }
    bytes
}

fn quoted(input: &str) -> String {
    let body = input
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{body}\"")
}

fn source_operations() -> Test<()> {
    let apply = |source: &str, rule: &str| -> Outcome<Vec<u8>> {
        let rules = format!(
            r#"{{"schema":1,"sources":{{"fixture.h":"{}"}},"files":{{"fixture.h":{rule}}}}}"#,
            sha256(source.as_bytes())?
        );
        let parsed = Pack::load(&pack(&rules))?;
        let inputs = Files::from([(b"fixture.h".to_vec(), source.as_bytes().to_vec())]);
        let mut first = parsed.transform(&inputs)?;
        check(
            first == parsed.transform(&inputs)?,
            INTERNAL,
            "deterministic source tree",
        )?;
        Ok(first.remove(b"fixture.h".as_slice()).expect("fixture file"))
    };
    let applied = |source, rule, wanted: &str| -> Test<()> {
        expect(
            apply(source, rule).map_err(describe)? == wanted.as_bytes(),
            "source operation result",
        )
    };
    let refused = |source, rule| {
        expect(
            apply(source, rule)
                .err()
                .is_some_and(|error: Refusal| error.code == ANCHOR),
            "missing or ambiguous anchor must refuse",
        )
    };
    let insert = r#"{"op":"inserts","edits":[{"at":19,"anchor_at":10,"anchor":"float4 p;","text":"float v;"}]}"#;
    applied(
        "struct O {float4 p;};",
        insert,
        "struct O {float4 p;float v;};",
    )?;
    refused("struct O {float4 q;};", insert)?;
    let extract = r##"{"op":"extract","source":"fixture.h","start":"float skin(","end":"#endif","prefix":"// RSF clone\n","suffix":"","remap":{"skin":"previous_skin","x":"old_x"}}"##;
    applied(
        "#define A 1
float skin(float x){return x*A;}
#endif
",
        extract,
        "// RSF clone
float previous_skin(float old_x){return old_x*A;}
",
    )?;
    refused(
        "#define A 1
#endif
",
        extract,
    )?;
    let clone = r#"{"op":"clone_function","start":"float wind(","prefix":"","before":"float wind(float x)","between":"\nfloat previous_wind(float old_x)","after":"","remap":{"x":"old_x"}}"#;
    applied(
        "float wind(float x){if(x){return x*2;}return 0;}
",
        clone,
        "float wind(float x){if(x){return x*2;}return 0;}
float previous_wind(float old_x){if(old_x){return old_x*2;}return 0;}
",
    )?;
    refused(
        "float wind(float x){return x;}
float wind(float y){return y;}
",
        clone,
    )?;
    refused(
        "float wind(float x){return x;
",
        clone,
    )
}

fn rewrite_pixel(pixel: &[u8]) -> Test<Vec<u8>> {
    let word = |offset| crate::dxbc_chunks::word(pixel, offset).ok_or("fixture bounds");
    let mut pattern = Vec::new();
    let mut replacement = Vec::new();
    for i in 0..word(28)? as usize {
        let at = word(32 + i * 4)? as usize;
        if !matches!(&pixel[at..at + 4], b"SHDR" | b"SHEX") {
            continue;
        }
        let end = at + 8 + word(at + 4)? as usize;
        let mut pos = at + 16;
        while pos < end {
            let size = (word(pos)? >> 24 & 127) as usize;
            expect(size != 0 && size <= (end - pos) / 4, "fixture opcode")?;
            for j in 1..size {
                if word(pos + j * 4)? == 0.25_f32.to_bits() {
                    pattern = (0..size)
                        .map(|i| word(pos + i * 4))
                        .collect::<Result<_, _>>()?;
                    replacement = pattern.clone();
                    replacement[j] = 0.5_f32.to_bits();
                }
            }
            pos += size * 4;
        }
    }
    expect(!pattern.is_empty(), "literal instruction pattern")?;
    let hash = sha256(pixel).map_err(describe)?;
    let rewritten =
        dxbc::rewrite(pixel, hash.as_bytes(), &pattern, &replacement).map_err(describe)?;
    expect(
        dxbc::rewrite(pixel, &[b'0'; 64], &pattern, &replacement)
            .err()
            .is_some_and(|error| error.code == HASH),
        "wrong hash must refuse",
    )?;
    Ok(rewritten)
}

fn counters(api: &rsf_shader_patch_api) -> Test<rsf_patch_counters> {
    let mut counters = rsf_patch_counters {
        struct_size: size_of::<rsf_patch_counters>() as u32,
        abi_version: VERSION,
        ..Default::default()
    };
    expect(
        unsafe { (api.counters.unwrap())(&mut counters) } == OK,
        "counter ABI",
    )?;
    expect(
        counters.built != 0 && counters.cache_hits != 0 && counters.refused >= 2,
        "counters",
    )?;
    Ok(counters)
}

fn render(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    vs: &ID3D11VertexShader,
    ps: &ID3D11PixelShader,
) -> Test<f32> {
    let mut desc = D3D11_TEXTURE2D_DESC {
        Width: 1,
        Height: 1,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
        ..Default::default()
    };
    let (mut target, mut copy, mut view) = (None, None, None);
    unsafe {
        win(device.CreateTexture2D(&desc, None, Some(&mut target)))?;
        let target = target.ok_or("render target")?;
        win(device.CreateRenderTargetView(&target, None, Some(&mut view)))?;
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        win(device.CreateTexture2D(&desc, None, Some(&mut copy)))?;
        let copy = copy.ok_or("readback texture")?;
        let no_depth = None::<&ID3D11DepthStencilView>;
        context.OMSetRenderTargets(Some(&[view]), no_depth);
        context.RSSetViewports(Some(&[D3D11_VIEWPORT {
            Width: 1.0,
            Height: 1.0,
            MaxDepth: 1.0,
            ..Default::default()
        }]));
        context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        context.VSSetShader(vs, None);
        context.PSSetShader(ps, None);
        context.Draw(3, 0);
        context.OMSetRenderTargets(None, no_depth);
        context.CopyResource(&copy, &target);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        win(context.Map(&copy, 0, D3D11_MAP_READ, 0, Some(&mut mapped)))?;
        let value = (!mapped.pData.is_null()).then(|| mapped.pData.cast::<f32>().read());
        context.Unmap(&copy, 0);
        value.ok_or_else(|| "readback map returned no data".into())
    }
}

fn warp(api: &rsf_shader_patch_api, stock: &[u8], pixel: &[u8], rewritten: &[u8]) -> Test<()> {
    let (device, context) = win(rsf_test_support::warp_d3d11(
        D3D11_CREATE_DEVICE_FLAG(0),
        None,
    ))?;
    unsafe {
        let d = device.as_raw();
        let c = context.as_raw();
        expect((api.attach_d3d11.unwrap())(d) == OK, "API interception")?;
        let (mut vs, mut old_ps) = (None, None);
        let no_linkage = None::<&ID3D11ClassLinkage>;
        win(device.CreateVertexShader(stock, no_linkage, Some(&mut vs)))?;
        win(device.CreatePixelShader(pixel, no_linkage, Some(&mut old_ps)))?;
        let vs = vs.ok_or("stock VS")?;
        let old_ps = old_ps.ok_or("stock PS")?;
        let mut new_ps = ptr::null_mut();
        expect(
            (api.create_d3d11.unwrap())(d, c"ps_5_0".as_ptr(), raw(rewritten), &mut new_ps) == OK
                && !new_ps.is_null(),
            "rewritten PS",
        )?;
        let new_ps = ID3D11PixelShader::from_raw(new_ps);
        expect(
            (render(&device, &context, &vs, &old_ps)? - 0.25).abs() < 1e-6
                && (render(&device, &context, &vs, &new_ps)? - 0.5).abs() < 1e-6,
            "original and rewritten GPU results",
        )?;

        let mut buffer = ptr::null_mut();
        expect(
            (api.constant_buffer.unwrap())(d, 16, &mut buffer) == OK && !buffer.is_null(),
            "generic buffer allocation",
        )?;
        let buffer = ID3D11Buffer::from_raw(buffer);
        let values: Vec<_> = [1.0_f32, 2.0, 3.0, 4.0]
            .into_iter()
            .flat_map(f32::to_ne_bytes)
            .collect();
        expect(
            (api.upload_constant.unwrap())(c, buffer.as_raw(), raw(&values)) == OK,
            "generic constant upload",
        )?;
        expect(
            (api.bind_constant.unwrap())(c, u32::from(b'v'), 13, buffer.as_raw()) == OK,
            "generic binding",
        )?;
        let mut bound = [None];
        context.VSGetConstantBuffers(13, Some(&mut bound));
        expect(
            bound[0]
                .as_ref()
                .is_some_and(|bound| bound.as_raw() == buffer.as_raw()),
            "binding readback",
        )?;
        expect(counters(api)?.intercepted >= 2, "shader creation counters")?;
        context.ClearState();
    }
    Ok(())
}

struct Cache(PathBuf);

impl Drop for Cache {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn synthetic(args: &[OsString]) -> Test<()> {
    let cpu_only = args.len() == 1 && args[0] == "--cpu";
    source_operations()?;
    let api = api()?;
    let mut short = api;
    short.struct_size = 0;
    expect(
        unsafe { rsf_get_shader_patch_api(&mut short) } == ARGUMENT,
        "short ABI",
    )?;
    short = api;
    short.abi_version = 99;
    expect(
        unsafe { rsf_get_shader_patch_api(&mut short) } == ABI,
        "wrong ABI",
    )?;
    let compiler = Compiler::system("d3dcompiler_47.dll").ok_or("fixture compiler unavailable")?;
    let entry = compiler
        .entry()
        .ok_or("fixture compiler export unavailable")?;
    let source =
        b"float4 main(uint id:SV_VertexID):SV_Position{return float4(id==2?3:-1,id==1?3:-1,0,1);}";
    let twin = [
        source.as_slice(),
        b"
",
    ]
    .concat();
    let mut originals = Files::from([
        (b"fixture.vs".to_vec(), source.to_vec()),
        (b"fixture2.vs".to_vec(), twin.clone()),
    ]);
    let suffix = "
#undef main
struct O{float4 p:SV_Position;float v:TEXCOORD12;};O main(uint id:SV_VertexID){O o;o.p=stock_main(id);o.v=1;return o;}";
    let wrap = |tail| {
        format!(
            r##"{{"op":"wrap","prefix":"#define main stock_main\n","suffix":{}}}"##,
            quoted(tail)
        )
    };
    let rules = format!(
        r#"{{"schema":1,"roots":[{{"name":"fixture.vs"}},{{"name":"fixture2.vs"}}],
        "sources":{{"fixture.vs":"{}","fixture2.vs":"{}"}},
        "files":{{"fixture.vs":{},"fixture2.vs":{}}},"allowed_bindings":[],"added_semantics":{{"TEXCOORD":[12]}}}}"#,
        sha256(source).map_err(describe)?,
        sha256(&twin).map_err(describe)?,
        wrap(suffix),
        wrap(&suffix.replace("o.v=1", "o.v=2")),
    );
    let blob = pack(&rules);
    let parsed = Pack::load(&blob).map_err(describe)?;
    let patched = parsed.transform(&originals).map_err(describe)?;
    expect(
        pack::find(&patched[b"fixture.vs".as_slice()], b"stock_main", 0).is_some(),
        "synthetic transform",
    )?;
    let cache =
        Cache(std::env::temp_dir().join(format!("rsf-shader-patch-{}", std::process::id())));
    let session = FixtureSession::open(&api, &blob, Some(&cache.0), entry)?;
    let mut rival = ptr::null_mut();
    let mut collision = blob.clone();
    let at = pack::find(&collision, b"synthetic", 0).ok_or("fixture pack identity")?;
    collision[at..at + 9].copy_from_slice(b"collision");
    expect(
        unsafe { (api.open.unwrap())(raw(&collision), ptr::null(), entry, &mut rival) } == FORMAT
            && rival.is_null(),
        "overlapping roots must refuse",
    )?;

    let mut request = super::request(&originals, source, c"vs_5_0", &[]);
    let stock = compile(entry, &request, &originals, source).map_err(describe)?;
    request.original_bytecode = raw(&stock);
    let mut result = output();
    session.prepare(&request, &mut result)?;
    let variant = bytes(result.bytecode).map_err(describe)?.to_vec();
    session.prepare(&request, &mut result)?;
    expect(result.cache_hit != 0, "verified cache hit")?;
    let stock2 = compile(entry, &request, &originals, &twin).map_err(describe)?;
    expect(
        stock2 == stock,
        "distinct source roots with identical original DXBC",
    )?;
    request.source = raw(&twin);
    session.prepare(&request, &mut result)?;
    expect(
        result.cache_hit == 0 && bytes(result.bytecode).map_err(describe)? != variant,
        "root-specific cache and variant",
    )?;
    request.source = raw(source);
    let mut changed = originals.clone();
    changed
        .values_mut()
        .next()
        .ok_or("fixture originals")?
        .push(b' ');
    request.user = (&raw mut changed).cast();
    expect(
        session.prepare_status(&request, &mut result) == HASH,
        "changed engine source must refuse",
    )?;
    request.user = (&raw mut originals).cast();
    request.original_bytecode = raw(&variant);
    expect(
        session.prepare_status(&request, &mut result) == ORIGINAL,
        "wrong engine DXBC must refuse",
    )?;
    drop(session);

    for size in 0..blob.len() {
        expect(
            Pack::load(&blob[..size])
                .err()
                .is_some_and(|error| error.code == FORMAT),
            "truncated pack must refuse",
        )?;
    }
    let pixel_source = b"float4 main():SV_Target{return float4(0.25,0.3,0.4,1);}";
    let request = super::request(&originals, pixel_source, c"ps_5_0", &[]);
    let pixel = compile(entry, &request, &originals, pixel_source).map_err(describe)?;
    let rewritten = rewrite_pixel(&pixel)?;
    counters(&api)?;
    if cpu_only {
        println!(
            "Synthetic pack/ABI/source proof/cache/refusal and DXBC rewrite passed without a device"
        );
    } else {
        warp(&api, &stock, &pixel, &rewritten)?;
        println!(
            "Synthetic pack/ABI/source proof/cache/refusal and WARP DXBC rewrite/interception/readback passed"
        );
    }
    Ok(())
}
