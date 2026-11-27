// SPDX-License-Identifier: MIT
//! Pack admission, source rules, reflection comparison and the DXBC rewrite.

use super::reflection::{Binding, Buffer, Parameter, Shader, compare};
use super::*;

fn le(out: &mut Vec<u8>, value: u64, n: usize) {
    out.extend_from_slice(&value.to_le_bytes()[..n]);
}

fn container(id: &str, rules: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    le(&mut payload, 1, 4);
    le(&mut payload, 12, 4);
    le(&mut payload, 7, 8);
    payload.extend_from_slice(b"metadata.txtfixture");
    let parts = [
        format!("{{\"id\":\"{id}\"}}").into_bytes(),
        rules.as_bytes().to_vec(),
        payload,
    ];
    let mut out = b"RSFPATCH".to_vec();
    le(&mut out, 1, 4);
    le(&mut out, 3, 4);
    le(&mut out, 24, 8);
    let mut offset = 96;
    for (i, part) in parts.iter().enumerate() {
        le(&mut out, i as u64 + 1, 4);
        le(&mut out, 0, 4);
        le(&mut out, offset, 8);
        le(&mut out, part.len() as u64, 8);
        offset += part.len() as u64;
    }
    parts.iter().for_each(|part| out.extend_from_slice(part));
    out
}

fn quoted(s: &[u8]) -> String {
    let mut out = String::from("\"");
    for &c in s {
        match c {
            b'\n' => out.push_str("\\n"),
            b'"' | b'\\' => {
                out.push('\\');
                out.push(c as char);
            }
            _ => out.push(c as char),
        }
    }
    out + "\""
}

fn hash(s: &[u8]) -> String {
    sha256(s).unwrap()
}

fn load(rules: &str) -> Outcome<Pack> {
    Pack::load(&container("fixture", rules))
}

fn transform_one(source: &[u8], rule: &str) -> Outcome<Vec<u8>> {
    let rules = format!(
        r#"{{"schema":1,"sources":{{"fixture.h":{}}},"files":{{"fixture.h":{rule}}}}}"#,
        quoted(hash(source).as_bytes())
    );
    let pack = load(&rules)?;
    let inputs = Files::from([(b"fixture.h".to_vec(), source.to_vec())]);
    let first = pack.transform(&inputs)?;
    assert_eq!(first, pack.transform(&inputs)?, "deterministic tree");
    assert_eq!(
        first[&b"metadata.txt"[..]],
        b"fixture",
        "authored payload added"
    );
    Ok(first[&b"fixture.h"[..]].clone())
}

fn code(result: Outcome<impl Sized>) -> i32 {
    result.err().map_or(OK, |refusal| refusal.code)
}

#[test]
fn pack_container_refuses_every_truncation_and_malformed_table() {
    let valid = container("fixture", r#"{"schema":1}"#);
    assert!(Pack::load(&valid).is_ok());
    for size in 0..valid.len() {
        assert_eq!(
            code(Pack::load(&valid[..size])),
            FORMAT,
            "truncated to {size}"
        );
    }
    let mut overlapping = valid.clone();
    overlapping[24 + 8] = 90; // metadata begins inside the section table
    assert_eq!(code(Pack::load(&overlapping)), FORMAT);
    let mut reserved = valid.clone();
    reserved[24 + 4] = 1;
    assert_eq!(code(Pack::load(&reserved)), FORMAT);
    let mut duplicate = valid.clone();
    duplicate[48] = 1; // the rules section claims to be metadata
    assert_eq!(code(Pack::load(&duplicate)), FORMAT);
    let mut version = valid;
    version[8] = 2;
    assert_eq!(code(Pack::load(&version)), FORMAT);
    assert_eq!(code(load(r#"{"schema":2}"#)), FORMAT, "rule schema");
    assert_eq!(
        code(load(r#"{"schema":-1}"#)),
        FORMAT,
        "a signed schema reads as its bit pattern"
    );
    assert_eq!(
        code(Pack::load(&container("", r#"{"schema":1}"#))),
        FORMAT,
        "empty id"
    );
}

#[test]
fn json_admission_refuses_duplicates_nul_keys_and_deep_nesting() {
    assert_eq!(code(load(r#"{"schema":1,"schema":1}"#)), FORMAT);
    assert_eq!(code(load(r#"{"schema":1,"a":{"k":1,"k":2}}"#)), FORMAT);
    assert_eq!(code(load(r#"{"schema":1,"a\u0000":1}"#)), FORMAT);
    assert_eq!(code(load(r#"{"schema":1,}"#)), FORMAT, "trailing comma");
    assert_eq!(
        code(load(r#"{"schema":1,"a":"\q"}"#)),
        FORMAT,
        "invalid escape"
    );
    let escaped = load(r#"{"schema":1,"a":"\uD83D\uDE00"}"#).unwrap();
    assert_eq!(
        text(Some(&escaped.rules), b"a").unwrap(),
        "\u{1f600}".as_bytes()
    );
    let nested = |depth: usize| {
        format!(
            r#"{{"schema":1,"x":{}0{}}}"#,
            "[".repeat(depth),
            "]".repeat(depth)
        )
    };
    assert!(
        load(&nested(126)).is_ok(),
        "a value 127 levels below the root"
    );
    assert_eq!(
        code(load(&nested(127))),
        FORMAT,
        "a value 128 levels below the root"
    );
}

#[test]
fn source_operations_match_the_authored_rules() {
    let insert = r#"{"op":"inserts","edits":[{"at":19,"anchor_at":10,"anchor":"float4 p;","text":"float v;"}]}"#;
    assert_eq!(
        transform_one(b"struct O {float4 p;};", insert).unwrap(),
        b"struct O {float4 p;float v;};"
    );
    assert_eq!(
        code(transform_one(b"struct O {float4 q;};", insert)),
        ANCHOR
    );
    let ordered = r#"{"op":"inserts","edits":[{"at":3,"anchor_at":0,"anchor":"a","text":"X"},
        {"at":1,"anchor_at":0,"anchor":"a","text":"Y"}]}"#;
    assert_eq!(
        transform_one(b"abc", ordered).unwrap(),
        b"aYbcX",
        "last offset first"
    );
    let unordered = ordered.replace("\"at\":1", "\"at\":3");
    assert_eq!(
        code(transform_one(b"abc", &unordered)),
        ANCHOR,
        "offsets must descend"
    );
    // Inside the grown text but beyond the original. The C++ owner threw out_of_range here.
    let past = r#"{"op":"inserts","edits":[{"at":3,"anchor_at":0,"anchor":"a","text":"XYZ"},
        {"at":2,"anchor_at":5,"anchor":"","text":""}]}"#;
    assert_eq!(
        code(transform_one(b"abc", past)),
        ANCHOR,
        "anchor beyond the original"
    );

    let extract = r##"{"op":"extract","source":"fixture.h","start":"float skin(","end":"#endif",
        "prefix":"// RSF clone\n","suffix":"","remap":{"skin":"previous_skin","x":"old_x"}}"##;
    let source = b"#define A 1\nfloat skin(float x){return x*A;}\n#endif\n";
    assert_eq!(
        transform_one(source, extract).unwrap(),
        b"// RSF clone\nfloat previous_skin(float old_x){return old_x*A;}\n"
    );
    assert_eq!(
        code(transform_one(b"#define A 1\n#endif\n", extract)),
        ANCHOR
    );

    let clone = r#"{"op":"clone_function","start":"float wind(","prefix":"","before":"float wind(float x)",
        "between":"\nfloat previous_wind(float old_x)","after":"","remap":{"x":"old_x"}}"#;
    assert_eq!(
        transform_one(b"\xef\xbb\xbffloat wind(float x){if(x){return x*2;}return 0;}\n", clone).unwrap(),
        b"float wind(float x){if(x){return x*2;}return 0;}\nfloat previous_wind(float old_x){if(old_x){return old_x*2;}return 0;}\n"
    );
    assert_eq!(
        code(transform_one(
            b"float wind(float x){return x;}\nfloat wind(float y){return y;}\n",
            clone
        )),
        ANCHOR
    );
    assert_eq!(
        code(transform_one(b"float wind(float x){return x;\n", clone)),
        ANCHOR
    );

    let wrap = r#"{"op":"wrap","prefix":"<","suffix":">"}"#;
    assert_eq!(transform_one(b"body", wrap).unwrap(), b"<body>");
    assert_eq!(code(transform_one(b"body", r#"{"op":"rename"}"#)), FORMAT);
    assert_eq!(
        code(transform_one(
            b"body",
            r#"{"op":"wrap","prefix":1,"suffix":""}"#
        )),
        FORMAT
    );
    let no_table =
        r#"{"op":"extract","source":"fixture.h","start":"b","end":"y","prefix":"","suffix":""}"#;
    assert_eq!(
        code(transform_one(b"body", no_table)),
        FORMAT,
        "remap table required"
    );
}

#[test]
fn source_hashes_inputs_and_payload_are_checked() {
    let rules = format!(
        r#"{{"schema":1,"sources":{{"a.h":{}}},"files":{{}}}}"#,
        quoted(hash(b"a").as_bytes())
    );
    let pack = load(&rules).unwrap();
    let tree = |name: &[u8], data: &[u8]| Files::from([(name.to_vec(), data.to_vec())]);
    assert!(pack.transform(&tree(b"a.h", b"a")).is_ok());
    assert_eq!(code(pack.transform(&tree(b"a.h", b"a\n"))), HASH);
    assert_eq!(code(pack.transform(&tree(b"b.h", b"a"))), SOURCE);
    let colliding = tree(b"metadata.txt", b"engine");
    let rules = r#"{"schema":1,"sources":{},"files":{}}"#;
    assert_eq!(
        code(load(rules).unwrap().transform(&colliding)),
        FORMAT,
        "payload never overrides a source"
    );
}

#[test]
fn reflection_admits_only_declared_additions() {
    let rules = |json: &str| pack::json(json.as_bytes()).unwrap();
    let declared = rules(
        r#"{"allowed_bindings":[{"name":"rsf_motion","point":13,"type":0,"count":1}],"added_semantics":{"TEXCOORD":[12]}}"#,
    );
    let position = Parameter {
        semantic: b"SV_POSITION".to_vec(),
        mask: 15,
        ..Default::default()
    };
    let stock = Shader {
        version: 0x50,
        outputs: vec![position.clone()],
        ..Default::default()
    };
    let mut variant = stock.clone();
    variant.outputs[0].semantic = b"sv_position".to_vec();
    variant.outputs.push(Parameter {
        semantic: b"TexCoord".to_vec(),
        index: 12,
        ..Default::default()
    });
    variant.bindings.push(Binding {
        name: b"rsf_motion".to_vec(),
        point: 13,
        count: 1,
        ..Default::default()
    });
    assert!(compare(&declared, &stock, &variant).is_ok());
    assert_eq!(code(compare(&rules("{}"), &stock, &variant)), REFLECTION);
    let malformed =
        rules(r#"{"allowed_bindings":[{"name":5}],"added_semantics":{"TEXCOORD":[12]}}"#);
    assert_eq!(
        code(compare(&malformed, &stock, &variant)),
        FORMAT,
        "pack error, not a reflection refusal"
    );

    let mut moved = variant.clone();
    moved.bindings[0].point = 12;
    assert_eq!(code(compare(&declared, &stock, &moved)), REFLECTION);
    let mut index = variant.clone();
    index.outputs[1].index = 11;
    assert_eq!(code(compare(&declared, &stock, &index)), REFLECTION);
    let mut model = variant.clone();
    model.version = 0x40;
    assert_eq!(code(compare(&declared, &stock, &model)), REFLECTION);

    let mut native = stock.clone();
    native.bindings.push(Binding {
        name: b"t0".to_vec(),
        kind: 2,
        ..Default::default()
    });
    native.buffers.push(Buffer {
        name: b"cb".to_vec(),
        size: 32,
        variables: vec![(b"m".to_vec(), 16, 16)],
    });
    native.patch_constants.push(Parameter {
        semantic: b"SV_TessFactor".to_vec(),
        ..Default::default()
    });
    assert!(compare(&declared, &native, &native.clone()).is_ok());
    let mut changed = native.clone();
    changed.bindings[0].dimension = 4;
    assert_eq!(code(compare(&declared, &native, &changed)), REFLECTION);
    let mut layout = native.clone();
    layout.buffers[0].variables[0].1 = 0;
    assert_eq!(code(compare(&declared, &native, &layout)), REFLECTION);
    let mut patch = native.clone();
    patch.patch_constants[0].semantic = b"SV_TESSFACTOR".to_vec();
    assert_eq!(
        code(compare(&declared, &native, &patch)),
        REFLECTION,
        "patch names are case-sensitive"
    );
}

#[test]
fn dxbc_rewrite_replaces_one_instruction_and_repairs_the_checksum() {
    for (i, k) in dxbc::K.iter().enumerate() {
        let expected = ((i as f64 + 1.0).sin().abs() * 4294967296.0).floor() as u32;
        assert_eq!(*k, expected);
    }
    // Header, one SHEX chunk: version, length, mov o0, l(0.25), ret.
    let program: [u32; 8] = [
        0x50,
        8,
        0x0500_0036,
        0x0010_2000,
        0,
        0x3e80_0000,
        0,
        0x0100_003e,
    ];
    let chunk_size = (program.len() * 4) as u32;
    let total = 32 + 4 + 8 + chunk_size;
    let mut container = b"DXBC".to_vec();
    container.extend_from_slice(&[0; 16]);
    for word in [1, total, 1, 36] {
        container.extend_from_slice(&u32::to_le_bytes(word));
    }
    container.extend_from_slice(b"SHEX");
    container.extend_from_slice(&chunk_size.to_le_bytes());
    for word in program {
        container.extend_from_slice(&word.to_le_bytes());
    }
    let sum = dxbc::checksum(&container[20..]).unwrap();
    for (bytes, word) in container[4..20].chunks_exact_mut(4).zip(sum) {
        bytes.copy_from_slice(&word.to_le_bytes());
    }
    let pinned = hash(&container);
    let mov = &program[2..7];
    let mut half = mov.to_vec();
    half[3] = 0x3f00_0000;
    let rewritten = dxbc::rewrite(&container, pinned.as_bytes(), mov, &half).unwrap();
    assert_eq!(rewritten.len(), container.len());
    assert_eq!(&rewritten[44 + 20..44 + 24], &0x3f00_0000u32.to_le_bytes());
    let sum = dxbc::checksum(&rewritten[20..]).unwrap();
    let stored: Vec<u32> = rewritten[4..20]
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert_eq!(stored, sum, "checksum repaired");
    assert_ne!(&rewritten[4..20], &container[4..20]);

    assert_eq!(
        code(dxbc::rewrite(&container, &[b'0'; 64], mov, &half)),
        HASH
    );
    assert_eq!(
        code(dxbc::rewrite(
            &container,
            pinned.as_bytes(),
            &mov[..4],
            &half[..4]
        )),
        ARGUMENT
    );
    let mut corrupt = container.clone();
    corrupt[4] ^= 1;
    assert_eq!(
        code(dxbc::rewrite(
            &corrupt,
            hash(&corrupt).as_bytes(),
            mov,
            &half
        )),
        FORMAT
    );
    let ret = &program[7..8];
    assert!(dxbc::rewrite(&container, pinned.as_bytes(), ret, ret).is_ok());
    let missing = [0x0100_003f];
    assert_eq!(
        code(dxbc::rewrite(
            &container,
            pinned.as_bytes(),
            &missing,
            &missing
        )),
        ANCHOR
    );
}
