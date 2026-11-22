// SPDX-License-Identifier: MIT
//! Owner tests with production JSON parsing and fake compiler and device leaves.

use super::leaf::*;
use super::reflection::{Binding, Buffer, Parameter, Shader, compare};
use super::*;
use crate::shader_patch_abi::{rsf_patch_define, rsf_patch_json_visit};
use std::ffi::CStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::AtomicUsize;

unsafe extern "C" {
    fn rsf_patch_parse_json(
        text: rsf_patch_bytes,
        user: *mut c_void,
        visit: rsf_patch_json_visit,
    ) -> i32;
}

pub(super) const DYNAMIC_USAGE: u32 = 2;
pub(super) const CPU_WRITE: u32 = 0x10000;
pub(super) const CONSTANT_SLOTS: u32 = 14;

const E_FAIL: i32 = 0x8000_4005_u32 as i32;

/// Output is `target|defines|source` with each `#include "name"` line expanded.
unsafe fn fake_compile(call: *const CompileCall) -> i32 {
    let call = unsafe { &*call };
    let source = bytes(call.source).unwrap_or_default();
    let fail = |text: &[u8]| {
        unsafe { (call.errors.unwrap())(call.user, text.as_ptr().cast(), text.len()) };
        E_FAIL
    };
    if pack::find(source, b"compile_error", 0).is_some() {
        return fail(b"fake error\0hidden");
    }
    let mut out = unsafe { c_bytes(call.target) }.to_vec();
    out.push(b'|');
    for i in 0..call.define_count {
        let d = unsafe { &*call.defines.add(i) };
        out.extend_from_slice(unsafe { c_bytes(d.name) });
        out.push(b'=');
        out.extend_from_slice(unsafe { c_bytes(d.value) });
        out.push(b';');
    }
    out.push(b'|');
    for line in source.split_inclusive(|&c| c == b'\n') {
        let Some(name) = line.strip_prefix(b"#include \"") else {
            out.extend_from_slice(line);
            continue;
        };
        let name = CString::new(name.split(|&c| c == b'"').next().unwrap_or_default()).unwrap();
        let (mut data, mut size) = (ptr::null(), 0);
        if unsafe { (call.include.unwrap())(call.user, name.as_ptr(), &mut data, &mut size) } != 0 {
            return fail(b"missing include");
        }
        out.extend_from_slice(&copy(data, size));
    }
    unsafe { (call.code.unwrap())(call.user, out.as_ptr().cast(), out.len()) };
    0
}

/// Every shader has one position output; marker text adds facts.
unsafe fn fake_reflect(code: rsf_patch_bytes, sink: *const ReflectionSink) -> i32 {
    let sink = unsafe { &*sink };
    let code = bytes(code).unwrap_or_default();
    let has = |marker: &[u8]| pack::find(code, marker, 0).is_some();
    if has(b"reflect_fail") {
        return -1;
    }
    let parameter = |semantic: &CStr, index: u32| ParameterRecord {
        semantic: semantic.as_ptr(),
        index,
        reg: index,
        system_value: 0,
        component_type: 3,
        mask: 15,
    };
    unsafe {
        (sink.shader.unwrap())(sink.user, 0x50);
        let position = parameter(c"SV_Position", 0);
        (sink.parameter.unwrap())(sink.user, RSF_PATCH_SIGNATURE_OUTPUT as u32, &position);
        if has(b"TEXCOORD12") {
            let texcoord = parameter(c"TEXCOORD", 12);
            (sink.parameter.unwrap())(sink.user, RSF_PATCH_SIGNATURE_OUTPUT as u32, &texcoord);
        }
        if has(b"b13") {
            let binding = BindingRecord {
                name: c"rsf_motion".as_ptr(),
                type_: 0,
                point: 13,
                count: 1,
                return_type: 0,
                dimension: 0,
            };
            (sink.binding.unwrap())(sink.user, &binding);
        }
    }
    0
}

unsafe fn fake_disassemble(code: rsf_patch_bytes, user: *mut c_void, text: Sink) -> i32 {
    let asm = [b"asm:", bytes(code).unwrap_or_default()].concat();
    unsafe { (text.unwrap())(user, asm.as_ptr().cast(), asm.len()) };
    0
}

// The fake device method calls the hooked pixel entry point, as a driver vtable would.
unsafe fn fake_create_shader(
    device: *mut c_void,
    _: u32,
    code: rsf_patch_bytes,
    out: *mut *mut c_void,
) -> i32 {
    unsafe { d3d11::DETOURS[1](device, code.data, code.size, ptr::null_mut(), out) }
}

unsafe fn fake_buffer(_: *mut c_void, _: u32, out: *mut *mut c_void) -> i32 {
    unsafe { *out = ptr::dangling_mut() };
    0
}

unsafe fn fake_desc(_: *mut c_void, width: *mut u32, usage: *mut u32, access: *mut u32) {
    unsafe {
        *width = 16;
        *usage = DYNAMIC_USAGE;
        *access = CPU_WRITE;
    }
}

static UPLOADS: AtomicUsize = AtomicUsize::new(0);
static BINDS: AtomicUsize = AtomicUsize::new(0);
static DISABLED: AtomicUsize = AtomicUsize::new(0);
static FAIL_ENABLE: AtomicUsize = AtomicUsize::new(0);
static REMOVED: AtomicUsize = AtomicUsize::new(0);

unsafe fn fake_upload(_: *mut c_void, _: *mut c_void, _: rsf_patch_bytes) -> i32 {
    UPLOADS.fetch_add(1, Ordering::Relaxed);
    0
}

unsafe fn fake_bind(_: *mut c_void, _: u32, _: u32, _: *mut c_void) {
    BINDS.fetch_add(1, Ordering::Relaxed);
}

unsafe fn fake_ready() -> i32 {
    1
}

unsafe extern "system" fn fake_original(
    _: *mut c_void,
    _: *const c_void,
    _: usize,
    _: *mut c_void,
    out: *mut *mut c_void,
) -> i32 {
    unsafe { *out = ptr::dangling_mut() };
    7
}

unsafe fn fake_hook(target: *mut c_void, _: *mut c_void, original: *mut *mut c_void) -> i32 {
    // An entry point numbered 99 refuses to be hooked.
    if target as usize == 99 {
        return 0;
    }
    unsafe { *original = fake_original as *mut c_void };
    1
}

unsafe fn fake_enable(target: *mut c_void) -> i32 {
    i32::from(target as usize != FAIL_ENABLE.load(Ordering::Relaxed))
}

unsafe fn fake_disable(_: *mut c_void) {
    DISABLED.fetch_add(1, Ordering::Relaxed);
}

unsafe fn fake_remove(_: *mut c_void) {
    REMOVED.fetch_add(1, Ordering::Relaxed);
}

pub(super) static FAKE: Platform = Platform {
    parse_json: Some(rsf_patch_parse_json),
    compile: Some(fake_compile),
    reflect: Some(fake_reflect),
    disassemble: Some(fake_disassemble),
    create_shader: Some(fake_create_shader),
    create_constant_buffer: Some(fake_buffer),
    buffer_desc: Some(fake_desc),
    upload: Some(fake_upload),
    bind_constant: Some(fake_bind),
    hooks_ready: Some(fake_ready),
    hook_create: Some(fake_hook),
    hook_enable: Some(fake_enable),
    hook_disable: Some(fake_disable),
    hook_remove: Some(fake_remove),
};

fn api() -> rsf_shader_patch_api {
    let mut api = rsf_shader_patch_api {
        struct_size: size_of::<rsf_shader_patch_api>() as u32,
        abi_version: VERSION,
        ..Default::default()
    };
    assert_eq!(unsafe { rsf_get_shader_patch_api(&mut api) }, OK);
    api
}

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
    Pack::load(&FAKE, &container("fixture", rules))
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
    assert!(Pack::load(&FAKE, &valid).is_ok());
    for size in 0..valid.len() {
        assert_eq!(
            code(Pack::load(&FAKE, &valid[..size])),
            FORMAT,
            "truncated to {size}"
        );
    }
    let mut overlapping = valid.clone();
    overlapping[24 + 8] = 90; // metadata begins inside the section table
    assert_eq!(code(Pack::load(&FAKE, &overlapping)), FORMAT);
    let mut reserved = valid.clone();
    reserved[24 + 4] = 1;
    assert_eq!(code(Pack::load(&FAKE, &reserved)), FORMAT);
    let mut duplicate = valid.clone();
    duplicate[48] = 1; // the rules section claims to be metadata
    assert_eq!(code(Pack::load(&FAKE, &duplicate)), FORMAT);
    let mut version = valid;
    version[8] = 2;
    assert_eq!(code(Pack::load(&FAKE, &version)), FORMAT);
    assert_eq!(code(load(r#"{"schema":2}"#)), FORMAT, "rule schema");
    assert_eq!(
        code(load(r#"{"schema":-1}"#)),
        FORMAT,
        "a signed schema reads as its bit pattern"
    );
    assert_eq!(
        code(Pack::load(&FAKE, &container("", r#"{"schema":1}"#))),
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
    let rules = |json: &str| pack::json(&FAKE, json.as_bytes()).unwrap();
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

struct Engine {
    files: Files,
}

unsafe extern "C" fn resolve(
    user: *mut c_void,
    name: *const c_char,
    out: *mut rsf_patch_bytes,
) -> i32 {
    let engine = unsafe { &*user.cast::<Engine>() };
    match engine.files.get(unsafe { c_bytes(name) }) {
        Some(data) => {
            unsafe { *out = raw(data) };
            OK
        }
        None => SOURCE,
    }
}

fn stock(source: &[u8], defines: &[(&str, &str)], includes: &Files) -> Vec<u8> {
    let mut out = b"vs_5_0|".to_vec();
    for (name, value) in defines {
        out.extend_from_slice(format!("{name}={value};").as_bytes());
    }
    out.push(b'|');
    for line in source.split_inclusive(|&c| c == b'\n') {
        match line.strip_prefix(b"#include \"") {
            Some(name) => {
                out.extend_from_slice(&includes[name.split(|&c| c == b'"').next().unwrap()])
            }
            None => out.extend_from_slice(line),
        }
    }
    out
}

fn request<'a>(
    engine: &'a Engine,
    source: &'a [u8],
    stock: &'a [u8],
    defines: &'a [rsf_patch_define],
) -> rsf_patch_request {
    rsf_patch_request {
        struct_size: size_of::<rsf_patch_request>() as u32,
        abi_version: VERSION,
        source: raw(source),
        original_bytecode: raw(stock),
        entry: c"main".as_ptr(),
        target: c"vs_5_0".as_ptr(),
        flags: 0,
        defines: defines.as_ptr(),
        define_count: defines.len(),
        resolve: Some(resolve),
        user: ptr::from_ref(engine).cast_mut().cast(),
    }
}

fn output() -> rsf_patch_output {
    rsf_patch_output {
        struct_size: size_of::<rsf_patch_output>() as u32,
        abi_version: VERSION,
        ..Default::default()
    }
}

fn reason(out: &rsf_patch_output) -> String {
    let bytes: Vec<u8> = out
        .reason
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8(bytes).unwrap()
}

fn compiler() -> *mut c_void {
    fake_original as *mut c_void
}

#[test]
fn sessions_prove_originals_reuse_verified_cache_and_count_refusals() {
    let api = api();
    let root = b"#include \"Shared.h\"\nfloat4 main(){return p;}\n".to_vec();
    let twin = [root.as_slice(), b"\n"].concat();
    let shared = b"// session fixture\n".to_vec();
    let engine = Engine {
        files: Files::from([
            (b"session.vs".to_vec(), root.clone()),
            (b"twin.vs".to_vec(), twin.clone()),
            (b"shared.h".to_vec(), shared.clone()),
        ]),
    };
    let wrap = r#"{"op":"wrap","prefix":"","suffix":"\nTEXCOORD12 b13\n"}"#;
    let patched_twin = hash(&[twin.as_slice(), b"\nTEXCOORD12 b13\n"].concat());
    // The first root has optional metadata and no authored patched hash.
    let rules = format!(
        r#"{{"schema":1,"roots":[{{"name":"session.vs","tag":"core","stage":"v"}},
        {{"name":"twin.vs","patched_sha256":"{patched_twin}","tag":"\u00e9","stage":"\u00ff"}}],
        "sources":{{"session.vs":{},"twin.vs":{},"shared.h":{}}},
        "files":{{"session.vs":{wrap},"twin.vs":{wrap}}},
        "allowed_bindings":[{{"name":"rsf_motion","point":13,"type":0,"count":1}}],
        "added_semantics":{{"TEXCOORD":[12]}}}}"#,
        quoted(hash(&root).as_bytes()),
        quoted(hash(&twin).as_bytes()),
        quoted(hash(&shared).as_bytes())
    );
    let blob = container("session", &rules);
    let cache = std::env::temp_dir().join(format!("rsf-shader-patch-rust-{}", std::process::id()));
    let cache_wide: Vec<u16> = cache.as_os_str().encode_wide().chain([0]).collect();
    let mut session = ptr::null_mut();
    let opened =
        unsafe { (api.open.unwrap())(raw(&blob), cache_wide.as_ptr(), compiler(), &mut session) };
    assert_eq!(opened, OK);
    assert_eq!(unsafe { (api.root_count.unwrap())(session) }, 2);
    let mut entry = rsf_patch_root {
        struct_size: size_of::<rsf_patch_root>() as u32,
        abi_version: VERSION,
        ..Default::default()
    };
    assert_eq!(
        unsafe { (api.root.unwrap())(session, 0, &mut entry) },
        OK,
        "missing optional root field"
    );
    assert_eq!(
        unsafe { c_bytes(entry.source_sha256) },
        hash(&root).as_bytes()
    );
    assert_eq!(unsafe { c_bytes(entry.patched_sha256) }, b"");
    assert_eq!(unsafe { (api.root.unwrap())(session, 1, &mut entry) }, OK);
    assert_eq!(unsafe { c_bytes(entry.tag) }, "\u{e9}".as_bytes());
    assert_eq!(
        entry.stage, 0xffff_ffc3,
        "first UTF-8 byte of U+00FF, sign-extended"
    );
    assert_eq!(
        unsafe { (api.root.unwrap())(session, 2, &mut entry) },
        ARGUMENT
    );
    let pack_hash = unsafe { c_bytes((api.hash.unwrap())(session)) };
    assert_eq!(pack_hash, hash(&blob).as_bytes());

    let rival = container("rival", &rules);
    let mut later = ptr::null_mut();
    let refused = unsafe { (api.open.unwrap())(raw(&rival), ptr::null(), compiler(), &mut later) };
    assert_eq!(
        (refused, later.is_null()),
        (FORMAT, true),
        "another pack claiming the same source"
    );
    let mut reload = ptr::null_mut();
    assert_eq!(
        unsafe { (api.open.unwrap())(raw(&blob), ptr::null(), compiler(), &mut reload) },
        OK
    );

    let defines = [rsf_patch_define {
        name: c"SM_5".as_ptr(),
        value: c"1".as_ptr(),
    }];
    let includes = Files::from([(b"Shared.h".to_vec(), shared.clone())]);
    let original = stock(&root, &[("SM_5", "1")], &includes);
    let call = request(&engine, &root, &original, &defines);
    let before = {
        let mut c = rsf_patch_counters {
            struct_size: size_of::<rsf_patch_counters>() as u32,
            abi_version: VERSION,
            ..Default::default()
        };
        assert_eq!(unsafe { (api.counters.unwrap())(&mut c) }, OK);
        c
    };
    let mut out = output();
    assert_eq!(
        unsafe { (api.prepare.unwrap())(session, &call, &mut out) },
        OK,
        "{}",
        reason(&out)
    );
    assert_eq!(out.cache_hit, 0);
    let variant = bytes(out.bytecode).unwrap().to_vec();
    assert!(pack::find(&variant, b"TEXCOORD12 b13", 0).is_some());
    assert!(
        pack::find(&variant, b"session fixture", 0).is_some(),
        "include resolved by canonical name"
    );
    assert_eq!(
        unsafe { (api.prepare.unwrap())(session, &call, &mut out) },
        OK
    );
    assert_eq!(
        (out.cache_hit, bytes(out.bytecode).unwrap()),
        (1, variant.as_slice()),
        "verified cache hit"
    );

    let twin_stock = stock(&twin, &[("SM_5", "1")], &includes);
    let call_twin = request(&engine, &twin, &twin_stock, &defines);
    assert_eq!(
        unsafe { (api.prepare.unwrap())(session, &call_twin, &mut out) },
        OK
    );
    assert_eq!(out.cache_hit, 0, "the cache key names the root");

    let wrong = request(&engine, &root, &variant, &defines);
    assert_eq!(
        unsafe { (api.prepare.unwrap())(session, &wrong, &mut out) },
        ORIGINAL
    );
    assert_eq!(reason(&out), "original does not reproduce engine DXBC");
    let unknown = request(&engine, b"float4 main();", &original, &defines);
    assert_eq!(
        unsafe { (api.prepare.unwrap())(session, &unknown, &mut out) },
        HASH
    );
    let changed = Engine {
        files: {
            let mut f = engine.files.clone();
            f.insert(b"shared.h".to_vec(), b"x".to_vec());
            f
        },
    };
    let stale = request(&changed, &root, &original, &defines);
    assert_eq!(
        unsafe { (api.prepare.unwrap())(session, &stale, &mut out) },
        HASH
    );
    let null_define = [rsf_patch_define {
        name: c"SM_5".as_ptr(),
        value: ptr::null(),
    }];
    let bad = request(&engine, &root, &original, &null_define);
    assert_eq!(
        unsafe { (api.prepare.unwrap())(session, &bad, &mut out) },
        ARGUMENT
    );
    let mut short = call;
    short.struct_size -= 1;
    assert_eq!(
        unsafe { (api.prepare.unwrap())(session, &short, &mut out) },
        ARGUMENT
    );
    let mut short_out = output();
    short_out.struct_size -= 1;
    assert_eq!(
        unsafe { (api.prepare.unwrap())(session, &call, &mut short_out) },
        ARGUMENT,
        "not counted"
    );

    let mut after = before;
    assert_eq!(unsafe { (api.counters.unwrap())(&mut after) }, OK);
    assert!(after.built >= before.built + 2 && after.cache_hits > before.cache_hits);
    assert!(after.refused >= before.refused + 5);
    assert!(after.refusals[7] > before.refusals[7] && after.refusals[4] >= before.refusals[4] + 2);

    unsafe { (api.close.unwrap())(session) };
    assert_eq!(
        unsafe { (api.open.unwrap())(raw(&rival), ptr::null(), compiler(), &mut later) },
        FORMAT,
        "reload still holds the claim"
    );
    unsafe { (api.close.unwrap())(reload) };
    assert_eq!(
        unsafe { (api.open.unwrap())(raw(&rival), ptr::null(), compiler(), &mut later) },
        OK,
        "claims released on close"
    );
    unsafe { (api.close.unwrap())(later) };
    let _ = fs::remove_dir_all(&cache);
}

#[test]
fn compile_failures_report_bounded_compiler_text() {
    let api = api();
    let root = b"compile_error\n".to_vec();
    let rules = format!(
        r#"{{"schema":1,"roots":[{{"name":"broken.vs"}}],"sources":{{"broken.vs":{}}},"files":{{}}}}"#,
        quoted(hash(&root).as_bytes())
    );
    let blob = container("broken", &rules);
    let mut session = ptr::null_mut();
    assert_eq!(
        unsafe { (api.open.unwrap())(raw(&blob), ptr::null(), compiler(), &mut session) },
        OK
    );
    let engine = Engine {
        files: Files::from([(b"broken.vs".to_vec(), root.clone())]),
    };
    let call = request(&engine, &root, b"", &[]);
    let mut out = output();
    assert_eq!(
        unsafe { (api.prepare.unwrap())(session, &call, &mut out) },
        COMPILE
    );
    assert_eq!(
        reason(&out),
        "fake error",
        "the reason stops at the compiler's NUL"
    );
    unsafe { (api.close.unwrap())(session) };
    let mut none = ptr::null_mut();
    assert_eq!(
        unsafe { (api.open.unwrap())(raw(&blob), ptr::null(), ptr::null_mut(), &mut none) },
        ARGUMENT
    );
    let foreign = ptr::dangling_mut::<c_void>();
    assert_eq!(
        unsafe { (api.open.unwrap())(raw(&blob), ptr::null(), foreign, &mut none) },
        ARGUMENT,
        "no compiler module"
    );
}

#[test]
fn query_and_resource_calls_validate_their_arguments() {
    let mut api = api();
    api.struct_size -= 1;
    assert_eq!(unsafe { rsf_get_shader_patch_api(&mut api) }, ARGUMENT);
    let mut api = self::api();
    api.abi_version = 2;
    assert_eq!(unsafe { rsf_get_shader_patch_api(&mut api) }, ABI);
    let api = self::api();
    let device = ptr::dangling_mut::<c_void>();
    let mut buffer = ptr::dangling_mut();
    assert_eq!(
        unsafe { (api.constant_buffer.unwrap())(device, 24, &mut buffer) },
        ARGUMENT
    );
    assert!(buffer.is_null(), "output cleared on refusal");
    assert_eq!(
        unsafe { (api.constant_buffer.unwrap())(device, 65552, &mut buffer) },
        ARGUMENT
    );
    assert_eq!(
        unsafe { (api.constant_buffer.unwrap())(device, 16, &mut buffer) },
        OK
    );
    let data = [0u8; 16];
    assert_eq!(
        unsafe { (api.upload_constant.unwrap())(device, buffer, raw(&data[..8])) },
        ARGUMENT
    );
    let uploads = UPLOADS.load(Ordering::Relaxed);
    assert_eq!(
        unsafe { (api.upload_constant.unwrap())(device, buffer, raw(&data)) },
        OK
    );
    assert_eq!(UPLOADS.load(Ordering::Relaxed), uploads + 1);
    let binds = BINDS.load(Ordering::Relaxed);
    assert_eq!(
        unsafe { (api.bind_constant.unwrap())(device, u32::from(b'x'), 13, buffer) },
        ARGUMENT
    );
    assert_eq!(
        unsafe { (api.bind_constant.unwrap())(device, u32::from(b'p'), 14, buffer) },
        ARGUMENT
    );
    assert_eq!(
        unsafe { (api.bind_constant.unwrap())(device, u32::from(b'p'), 13, ptr::null_mut()) },
        OK
    );
    assert_eq!(BINDS.load(Ordering::Relaxed), binds + 1);
    let mut shader = ptr::dangling_mut();
    let code = [1u8; 4];
    assert_eq!(
        unsafe { (api.create_d3d11.unwrap())(device, c"x_5_0".as_ptr(), raw(&code), &mut shader) },
        COMPILE
    );
    assert_eq!(
        unsafe { (api.create_d3d11.unwrap())(device, c"ps_5_0".as_ptr(), raw(&[]), &mut shader) },
        ARGUMENT
    );
}

#[test]
fn creation_hooks_inventory_engine_shaders_and_skip_replacements() {
    let api = api();
    let mut vtable = [0usize; 19];
    vtable
        .iter_mut()
        .enumerate()
        .for_each(|(i, slot)| *slot = 0x1000 + i);
    let device = [vtable.as_ptr() as usize];
    let device = device.as_ptr().cast_mut().cast::<c_void>();
    let mut refused_table = vtable;
    refused_table[13] = 99;
    let refused_device = [refused_table.as_ptr() as usize];
    let removed = REMOVED.load(Ordering::Relaxed);
    let refused = unsafe { (api.attach_d3d11.unwrap())(refused_device.as_ptr().cast_mut().cast()) };
    assert_eq!(refused, INTERNAL);
    assert_eq!(
        REMOVED.load(Ordering::Relaxed),
        removed + 2,
        "created hooks rolled back"
    );
    FAIL_ENABLE.store(vtable[13], Ordering::Relaxed);
    let disabled = DISABLED.load(Ordering::Relaxed);
    assert_eq!(unsafe { (api.attach_d3d11.unwrap())(device) }, INTERNAL);
    assert_eq!(DISABLED.load(Ordering::Relaxed), disabled + 2);
    assert_eq!(
        REMOVED.load(Ordering::Relaxed),
        removed + 2,
        "trampolines retained"
    );
    FAIL_ENABLE.store(0, Ordering::Relaxed);
    assert_eq!(unsafe { (api.attach_d3d11.unwrap())(device) }, OK);
    assert_eq!(
        unsafe { (api.attach_d3d11.unwrap())(device) },
        OK,
        "same entry points"
    );
    let mut other = vtable;
    other[15] += 1;
    let other_device = [other.as_ptr() as usize];
    assert_eq!(
        unsafe { (api.attach_d3d11.unwrap())(other_device.as_ptr().cast_mut().cast()) },
        ARGUMENT
    );

    let counters = || {
        let mut c = rsf_patch_counters {
            struct_size: size_of::<rsf_patch_counters>() as u32,
            abi_version: VERSION,
            ..Default::default()
        };
        unsafe { (api.counters.unwrap())(&mut c) };
        c.intercepted
    };
    let engine_code = b"engine pixel shader".to_vec();
    let before = counters();
    let mut shader = ptr::null_mut();
    let result = unsafe {
        d3d11::DETOURS[1](
            device,
            engine_code.as_ptr().cast(),
            engine_code.len(),
            ptr::null_mut(),
            &mut shader,
        )
    };
    assert_eq!(
        result, 7,
        "the original entry point still creates the shader"
    );
    assert!(counters() > before);
    assert!(d3d11::inventory_contains(&hash(&engine_code)));

    let replacement = b"replacement pixel shader".to_vec();
    let before = counters();
    assert_eq!(
        unsafe {
            (api.create_d3d11.unwrap())(device, c"ps_5_0".as_ptr(), raw(&replacement), &mut shader)
        },
        OK
    );
    assert_eq!(
        counters(),
        before,
        "the service's own creation is not an engine creation"
    );
    assert!(!d3d11::inventory_contains(&hash(&replacement)));

    let directory =
        std::env::temp_dir().join(format!("rsf-shader-dump-rust-{}", std::process::id()));
    let wide: Vec<u16> = directory.as_os_str().encode_wide().chain([0]).collect();
    assert_eq!(unsafe { (api.dump.unwrap())(wide.as_ptr()) }, OK);
    let name = hash(&engine_code);
    assert_eq!(
        fs::read(directory.join(format!("{name}.dxbc"))).unwrap(),
        engine_code
    );
    assert_eq!(
        fs::read(directory.join(format!("{name}.asm"))).unwrap(),
        [b"asm:", &engine_code[..]].concat()
    );
    let dxbc = directory.join(format!("{name}.dxbc"));
    fs::remove_file(&dxbc).unwrap();
    fs::create_dir(&dxbc).unwrap();
    assert_eq!(unsafe { (api.dump.unwrap())(wide.as_ptr()) }, INTERNAL);
    let report = fs::read_to_string(directory.join("inventory-report.txt")).unwrap();
    assert!(report.contains(&format!("{name}.dxbc")));
    assert_eq!(unsafe { (api.dump.unwrap())(ptr::null()) }, ARGUMENT);
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn short_records_are_refused_from_their_size_field_alone() {
    let api = api();
    // Each record is only its four-byte size field, followed by nothing the callee may read.
    let mut sizes = [4u32; 5];
    let [
        query,
        root_out,
        output_record,
        counters_record,
        request_record,
    ] = sizes.each_mut().map(ptr::from_mut);
    assert_eq!(unsafe { rsf_get_shader_patch_api(query.cast()) }, ARGUMENT);
    assert_eq!(
        unsafe { (api.counters.unwrap())(counters_record.cast()) },
        ARGUMENT
    );
    let root = b"short\n".to_vec();
    let rules = format!(
        r#"{{"schema":1,"roots":[{{"name":"short.vs"}}],"sources":{{"short.vs":{}}},"files":{{}}}}"#,
        quoted(hash(&root).as_bytes())
    );
    let blob = container("short", &rules);
    let mut session = ptr::null_mut();
    assert_eq!(
        unsafe { (api.open.unwrap())(raw(&blob), ptr::null(), compiler(), &mut session) },
        OK
    );
    assert_eq!(
        unsafe { (api.root.unwrap())(session, 0, root_out.cast()) },
        ARGUMENT
    );
    let call = request_record.cast::<rsf_patch_request>();
    assert_eq!(
        unsafe { (api.prepare.unwrap())(session, call, output_record.cast()) },
        ARGUMENT
    );
    let mut out = output();
    assert_eq!(
        unsafe { (api.prepare.unwrap())(session, call, &mut out) },
        ARGUMENT
    );
    assert_eq!(reason(&out), "request size");
    // Size is checked before version, and the version follows in a full record.
    let mut versioned = rsf_patch_counters {
        struct_size: size_of::<rsf_patch_counters>() as u32,
        abi_version: 2,
        ..Default::default()
    };
    assert_eq!(unsafe { (api.counters.unwrap())(&mut versioned) }, ABI);
    unsafe { (api.close.unwrap())(session) };
}

#[test]
fn cache_publishes_complete_files_and_reuses_them_across_sessions() {
    let api = api();
    let root = b"float4 cached(){return 0;}\n".to_vec();
    let rules = format!(
        r#"{{"schema":1,"roots":[{{"name":"cached.vs"}}],"sources":{{"cached.vs":{}}},
        "files":{{"cached.vs":{{"op":"wrap","prefix":"","suffix":"// variant\n"}}}}}}"#,
        quoted(hash(&root).as_bytes())
    );
    let blob = container("cached", &rules);
    let engine = Engine {
        files: Files::from([(b"cached.vs".to_vec(), root.clone())]),
    };
    let original = stock(&root, &[], &Files::new());
    let call = request(&engine, &root, &original, &[]);
    let directory =
        std::env::temp_dir().join(format!("rsf-shader-cache-rust-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    let open = |cache: &std::path::Path| {
        let wide: Vec<u16> = cache.as_os_str().encode_wide().chain([0]).collect();
        let mut session = ptr::null_mut();
        assert_eq!(
            unsafe { (api.open.unwrap())(raw(&blob), wide.as_ptr(), compiler(), &mut session) },
            OK
        );
        session
    };
    let prepare = |session| {
        let mut out = output();
        assert_eq!(
            unsafe { (api.prepare.unwrap())(session, &call, &mut out) },
            OK,
            "{}",
            reason(&out)
        );
        (out.cache_hit, bytes(out.bytecode).unwrap().to_vec())
    };

    let first = open(&directory);
    let (hit, variant) = prepare(first);
    assert_eq!(hit, 0);
    let files: Vec<_> = fs::read_dir(&directory)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(
        files.len(),
        1,
        "one published file and no temporary left behind"
    );
    assert_eq!(files[0].extension().unwrap(), "dxbc");
    assert_eq!(fs::read(&files[0]).unwrap(), variant);
    // A reload of the same pack reads the file the earlier session published.
    let reload = open(&directory);
    assert_eq!(prepare(reload), (1, variant.clone()));
    unsafe { (api.close.unwrap())(first) };
    // A file that fails admission is rebuilt and replaced.
    fs::write(&files[0], b"reflect_fail").unwrap();
    assert_eq!(prepare(reload), (0, variant.clone()));
    assert_eq!(fs::read(&files[0]).unwrap(), variant);
    unsafe { (api.close.unwrap())(reload) };

    // An unwritable cache still serves the variant and leaves nothing behind.
    let blocked = directory.join("blocked");
    fs::write(&blocked, b"a file, not a directory").unwrap();
    let session = open(&blocked);
    assert_eq!(prepare(session), (0, variant.clone()));
    assert_eq!(prepare(session), (0, variant), "nothing was published");
    unsafe { (api.close.unwrap())(session) };
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 2);
    let _ = fs::remove_dir_all(&directory);
}
