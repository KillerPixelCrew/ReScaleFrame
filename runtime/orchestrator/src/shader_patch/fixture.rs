// SPDX-License-Identifier: MIT
//! Command-line corpus and WARP fixtures, linked by their CMake test entry points.

mod device;

use super::*;
use crate::shader_patch_abi::rsf_patch_define;
use rsf_shader_compiler::Compiler;
use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

type Test<T> = Result<T, String>;

fn expect(ok: bool, message: &str) -> Test<()> {
    ok.then_some(()).ok_or_else(|| message.to_owned())
}

fn describe(error: Refusal) -> String {
    format!("{}: {}", error.code, String::from_utf8_lossy(&error.reason))
}

fn read(path: &Path) -> Test<Vec<u8>> {
    fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn collect_files(root: &Path, directory: &Path, files: &mut Files) -> Test<()> {
    let entries =
        fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            collect_files(root, &path, files)?;
        } else if path.is_file() {
            let name = path.strip_prefix(root).map_err(|error| error.to_string())?;
            let name = name
                .to_str()
                .ok_or_else(|| format!("invalid path: {}", name.display()))?;
            files.insert(name.replace('\\', "/").into_bytes(), read(&path)?);
        }
    }
    Ok(())
}

unsafe extern "C" fn resolve(
    user: *mut c_void,
    name: *const c_char,
    out: *mut rsf_patch_bytes,
) -> i32 {
    let files = unsafe { &*user.cast::<Files>() };
    let Some(data) = files.get(unsafe { c_bytes(name) }) else {
        return SOURCE;
    };
    unsafe { *out = raw(data) };
    OK
}

fn api() -> Test<rsf_shader_patch_api> {
    let mut api = rsf_shader_patch_api {
        struct_size: size_of::<rsf_shader_patch_api>() as u32,
        abi_version: VERSION,
        ..Default::default()
    };
    expect(
        unsafe { rsf_get_shader_patch_api(&mut api) } == OK,
        "API query",
    )?;
    Ok(api)
}

struct FixtureSession<'a> {
    api: &'a rsf_shader_patch_api,
    session: *mut rsf_patch_session,
}

impl Drop for FixtureSession<'_> {
    fn drop(&mut self) {
        unsafe { (self.api.close.unwrap())(self.session) };
    }
}

impl<'a> FixtureSession<'a> {
    fn open(
        api: &'a rsf_shader_patch_api,
        pack: &[u8],
        cache: Option<&Path>,
        compiler: *mut c_void,
    ) -> Test<Self> {
        let mut session = ptr::null_mut();
        let cache = cache.map(|path| {
            path.as_os_str()
                .encode_wide()
                .chain([0])
                .collect::<Vec<_>>()
        });
        let cache = cache.as_ref().map_or(ptr::null(), |path| path.as_ptr());
        let result = unsafe { (api.open.unwrap())(raw(pack), cache, compiler, &mut session) };
        expect(
            result == OK && !session.is_null(),
            &format!("open session ({result})"),
        )?;
        Ok(Self { api, session })
    }

    fn prepare_status(&self, request: &rsf_patch_request, output: &mut rsf_patch_output) -> i32 {
        unsafe { (self.api.prepare.unwrap())(self.session, request, output) }
    }

    fn prepare(&self, request: &rsf_patch_request, output: &mut rsf_patch_output) -> Test<()> {
        let result = self.prepare_status(request, output);
        if result == OK {
            Ok(())
        } else {
            let reason = unsafe { c_bytes(output.reason.as_ptr()) };
            Err(format!("{result}: {}", String::from_utf8_lossy(reason)))
        }
    }
}

fn output() -> rsf_patch_output {
    rsf_patch_output {
        struct_size: size_of::<rsf_patch_output>() as u32,
        abi_version: VERSION,
        ..Default::default()
    }
}

fn request(
    files: &Files,
    source: &[u8],
    target: &std::ffi::CStr,
    defines: &[rsf_patch_define],
) -> rsf_patch_request {
    rsf_patch_request {
        struct_size: size_of::<rsf_patch_request>() as u32,
        abi_version: VERSION,
        source: raw(source),
        original_bytecode: Default::default(),
        entry: c"main".as_ptr(),
        target: target.as_ptr(),
        flags: 0,
        defines: defines.as_ptr(),
        define_count: defines.len(),
        resolve: Some(resolve),
        user: (files as *const Files).cast_mut().cast(),
    }
}

fn compile_roots(pack: &[u8], files: &Files) -> Test<()> {
    let compiler = Compiler::system("d3dcompiler_43.dll").ok_or("engine compiler unavailable")?;
    let entry = compiler
        .entry()
        .ok_or("engine compiler export unavailable")?;
    let api = api()?;
    let session = FixtureSession::open(&api, pack, None, entry)?;
    let count = unsafe { (api.root_count.unwrap())(session.session) };
    let defines = [
        rsf_patch_define {
            name: c"SM_5".as_ptr(),
            value: c"1".as_ptr(),
        },
        rsf_patch_define {
            name: c"SKIN_1".as_ptr(),
            value: c"1".as_ptr(),
        },
    ];
    for i in 0..count {
        let mut root = rsf_patch_root {
            struct_size: size_of::<rsf_patch_root>() as u32,
            abi_version: VERSION,
            ..Default::default()
        };
        expect(
            unsafe { (api.root.unwrap())(session.session, i, &mut root) } == OK,
            "root metadata",
        )?;
        let name = unsafe { c_bytes(root.name) };
        let tag = unsafe { c_bytes(root.tag) };
        let mut target = *b"vs_5_0";
        target[0] = u8::try_from(root.stage).map_err(|_| "invalid root stage")?;
        let target = CString::new(target).map_err(|_| "invalid root stage")?;
        let source = files
            .get(name)
            .ok_or_else(|| format!("missing root {}", String::from_utf8_lossy(name)))?;
        let defines = if tag == b"core_model" {
            &defines[..]
        } else {
            &defines[..1]
        };
        let mut request = request(files, source, &target, defines);
        let stock = compile(platform(), entry, &request, files, source).map_err(describe)?;
        request.original_bytecode = raw(&stock);
        session
            .prepare(&request, &mut output())
            .map_err(|error| format!("{}: {error}", String::from_utf8_lossy(name)))?;
    }
    println!(
        "{count} engine-compiler roots source-proven, runtime-compiled and reflection-checked"
    );
    Ok(())
}

fn corpus(args: &[OsString]) -> Test<()> {
    expect(
        matches!(args.len(), 3 | 4),
        "expected pack, corpus, oracle, optional compile",
    )?;
    let pack_data = read(Path::new(&args[0]))?;
    let corpus = Path::new(&args[1]);
    let oracle = Path::new(&args[2]).join("motion").join("r3");
    let mut inputs = Files::new();
    collect_files(corpus, corpus, &mut inputs)?;
    let pack = Pack::load(platform(), &pack_data).map_err(describe)?;
    let output = pack.transform(&inputs).map_err(describe)?;
    let mut count = 0;
    for (name, data) in output {
        if name == b"metadata.txt" {
            continue;
        }
        let name = std::str::from_utf8(&name).map_err(|error| error.to_string())?;
        expect(
            data == read(&oracle.join(name))?,
            &format!("parity mismatch: {name}"),
        )?;
        count += 1;
    }
    println!("{count} files byte-identical to Python");
    if args.len() == 4 {
        compile_roots(&pack_data, &inputs)?;
    }
    inputs
        .values_mut()
        .next()
        .ok_or("empty corpus")?
        .push(b'\n');
    expect(
        pack.transform(&inputs)
            .err()
            .is_some_and(|error| error.code == HASH),
        "changed corpus must refuse",
    )?;
    for size in 0..24 {
        expect(
            Pack::load(platform(), &[b'x'; 24][..size]).is_err(),
            "truncated pack must refuse",
        )?;
    }
    Ok(())
}

fn run(test: impl FnOnce(&[OsString]) -> Test<()>) -> i32 {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match test(&args) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_shader_patch_corpus_main() -> i32 {
    run(corpus)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_shader_patch_device_main() -> i32 {
    run(device::synthetic)
}
