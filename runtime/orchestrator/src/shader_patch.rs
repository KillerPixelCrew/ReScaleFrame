// SPDX-License-Identifier: MIT
//! Owner of the Runtime shader patch service behind `rescaleframe/shader_patch.h`.
//!
//! One process-wide owner serves every SDK client: the source claim registry, counters, the
//! creation inventory and D3D11 hooks. Sessions own one admitted pack each.

mod d3d11;
#[cfg(any(test, feature = "test-fixtures"))]
mod dxbc;
#[cfg(all(feature = "test-fixtures", not(test)))]
mod fixture;
mod leaf;
mod pack;
#[cfg(not(test))]
mod platform;
mod reflection;
#[cfg(test)]
mod tests;

use crate::sr_session::has_capacity;
use crate::sync::lock;
use crate::sys::{
    RSF_PATCH_ABI, RSF_PATCH_ANCHOR, RSF_PATCH_ARGUMENT, RSF_PATCH_COMPILE, RSF_PATCH_FORMAT,
    RSF_PATCH_HASH, RSF_PATCH_INTERNAL, RSF_PATCH_OK, RSF_PATCH_ORIGINAL, RSF_PATCH_REFLECTION,
    RSF_PATCH_SOURCE, RSF_SHADER_PATCH_VERSION, rsf_patch_bytes, rsf_patch_counters,
    rsf_patch_output, rsf_patch_request, rsf_patch_root, rsf_patch_session, rsf_shader_patch_api,
};
use leaf::{CompileCall, Platform};
use pack::{Json, Pack, bytes, string, text};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ffi::{CString, c_char, c_void};
use std::os::windows::ffi::OsStringExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, ptr};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_SHA256_ALGORITHM, BCryptCloseAlgorithmProvider, BCryptHash,
    BCryptOpenAlgorithmProvider,
};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleFileNameW, GetModuleHandleExW,
};

const OK: i32 = RSF_PATCH_OK;
const ARGUMENT: i32 = RSF_PATCH_ARGUMENT;
const ABI: i32 = RSF_PATCH_ABI;
const SOURCE: i32 = RSF_PATCH_SOURCE;
const HASH: i32 = RSF_PATCH_HASH;
const ANCHOR: i32 = RSF_PATCH_ANCHOR;
const COMPILE: i32 = RSF_PATCH_COMPILE;
const ORIGINAL: i32 = RSF_PATCH_ORIGINAL;
const REFLECTION: i32 = RSF_PATCH_REFLECTION;
const FORMAT: i32 = RSF_PATCH_FORMAT;
const INTERNAL: i32 = RSF_PATCH_INTERNAL;
const VERSION: u32 = RSF_SHADER_PATCH_VERSION;
// Bump when transform semantics change within one product version.
const CACHE_PREFIX: &str = concat!("rsf-source-cache-1:", env!("CARGO_PKG_VERSION"), ":");
const COMPILE_ERROR_LIMIT: usize = 240;

/// Virtual file name to contents, ordered by bytes.
type Files = BTreeMap<Vec<u8>, Vec<u8>>;

/// A refused request: the public result code and a bounded reason for `rsf_patch_output`.
#[derive(Debug)]
struct Refusal {
    code: i32,
    reason: Cow<'static, [u8]>,
}

type Outcome<T> = Result<T, Refusal>;

fn refuse(code: i32, reason: &'static str) -> Refusal {
    Refusal {
        code,
        reason: Cow::Borrowed(reason.as_bytes()),
    }
}

fn check(ok: bool, code: i32, reason: &'static str) -> Outcome<()> {
    if ok {
        Ok(())
    } else {
        Err(refuse(code, reason))
    }
}

struct Counters {
    intercepted: AtomicU64,
    built: AtomicU64,
    cache_hits: AtomicU64,
    refused: AtomicU64,
    // Indexed by the negated result code.
    refusals: [AtomicU64; 11],
}

static COUNTERS: Counters = Counters {
    intercepted: AtomicU64::new(0),
    built: AtomicU64::new(0),
    cache_hits: AtomicU64::new(0),
    refused: AtomicU64::new(0),
    refusals: [const { AtomicU64::new(0) }; 11],
};

// Source hash to the claiming pack id and the number of live sessions of that pack.
type Owners = BTreeMap<Vec<u8>, (Vec<u8>, usize)>;
static OWNERS: Mutex<Owners> = Mutex::new(BTreeMap::new());

// Distinguishes concurrent cache writers within this process.
static TEMPORARY: AtomicU64 = AtomicU64::new(0);

fn platform() -> &'static Platform {
    #[cfg(not(test))]
    {
        &platform::PLATFORM
    }
    #[cfg(test)]
    {
        &tests::FAKE
    }
}

/// Admits a caller record by its leading `struct_size`, then its `abi_version`, before any
/// reference covers the whole record. A short caller may supply only the size field.
unsafe fn record<'a, T>(value: *mut T) -> Result<&'a mut T, i32> {
    if !unsafe { has_capacity(value) } {
        return Err(ARGUMENT);
    }
    if unsafe { value.cast::<u32>().add(1).read() } != VERSION {
        return Err(ABI);
    }
    Ok(unsafe { &mut *value })
}

/// Runs an entry point body, reporting a panic as an internal failure.
fn guarded(body: impl FnOnce() -> i32) -> i32 {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(INTERNAL)
}

/// A C view of owned bytes, null for an empty range.
fn raw(data: &[u8]) -> rsf_patch_bytes {
    rsf_patch_bytes {
        data: if data.is_empty() {
            ptr::null()
        } else {
            data.as_ptr().cast()
        },
        size: data.len(),
    }
}

unsafe fn wide_path(text: *const u16) -> PathBuf {
    let length = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    let wide = unsafe { std::slice::from_raw_parts(text, length) };
    PathBuf::from(std::ffi::OsString::from_wide(wide))
}

unsafe fn c_bytes<'a>(text: *const c_char) -> &'a [u8] {
    unsafe { std::ffi::CStr::from_ptr(text) }.to_bytes()
}

fn write_reason(out: &mut [c_char; 256], reason: &[u8]) {
    let reason = reason.split(|&b| b == 0).next().unwrap_or_default();
    let length = reason.len().min(out.len() - 1);
    for (dst, &src) in out.iter_mut().zip(&reason[..length]) {
        *dst = src as c_char;
    }
    out[length] = 0;
}

/// Lowercase SHA-256 hex digest.
fn sha256(data: &[u8]) -> Outcome<String> {
    check(
        data.len() <= u32::MAX as usize,
        ARGUMENT,
        "hash input too large",
    )?;
    let mut algorithm: BCRYPT_ALG_HANDLE = ptr::null_mut();
    let opened = unsafe {
        BCryptOpenAlgorithmProvider(&mut algorithm, BCRYPT_SHA256_ALGORITHM, ptr::null(), 0)
    };
    check(opened >= 0, INTERNAL, "SHA provider")?;
    let mut digest = [0u8; 32];
    let status = unsafe {
        BCryptHash(
            algorithm,
            ptr::null(),
            0,
            data.as_ptr(),
            data.len() as u32,
            digest.as_mut_ptr(),
            digest.len() as u32,
        )
    };
    unsafe { BCryptCloseAlgorithmProvider(algorithm, 0) };
    check(status >= 0, INTERNAL, "SHA failure")?;
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// Claims every allowlisted source for one pack. Sessions of the same pack id may overlap,
/// as a transactional reload does; another pack touching a claimed source is refused.
fn claim(id: &[u8], hashes: &[Vec<u8>]) -> Outcome<()> {
    let mut owners = lock(&OWNERS);
    let taken = hashes
        .iter()
        .any(|hash| owners.get(hash).is_some_and(|(owner, _)| owner != id));
    check(!taken, FORMAT, "shader already owned by another pack")?;
    for hash in hashes {
        owners
            .entry(hash.clone())
            .or_insert_with(|| (id.to_vec(), 0))
            .1 += 1;
    }
    Ok(())
}

fn unclaim(hashes: &[Vec<u8>]) {
    let mut owners = lock(&OWNERS);
    for hash in hashes {
        if let Some((_, count)) = owners.get_mut(hash) {
            *count -= 1;
            if *count == 0 {
                owners.remove(hash);
            }
        }
    }
}

/// Root metadata as C strings, borrowed by callers until the session closes.
struct Root {
    name: CString,
    source: CString,
    patched: CString,
    tag: CString,
    stage: u32,
}

fn admit_roots(pack: &Pack) -> Outcome<Vec<Root>> {
    let Some(Json::Arr(entries)) = pack.rules.get(b"roots") else {
        return Err(refuse(FORMAT, "root allowlist"));
    };
    let mut names = std::collections::BTreeSet::new();
    let mut roots = Vec::with_capacity(entries.len());
    for entry in entries {
        let name = text(Some(entry), b"name")?;
        check(names.insert(name), FORMAT, "duplicate root name")?;
        let source = text(pack.rules.get(b"sources"), name)?;
        let optional = |key: &[u8]| -> Outcome<&[u8]> {
            entry.get(key).map_or(Ok(&[]), |value| string(Some(value)))
        };
        if entry.get(b"original_sha256").is_some() {
            check(
                optional(b"original_sha256")? == source,
                FORMAT,
                "root source hash differs from source list",
            )?;
        }
        let patched = optional(b"patched_sha256")?;
        if entry.get(b"patched_sha256").is_some() {
            check(
                patched.len() == 64 && patched.iter().all(u8::is_ascii_hexdigit),
                FORMAT,
                "invalid patched root hash",
            )?;
        }
        let stage = optional(b"stage")?.first().copied().unwrap_or_default();
        let c_string =
            |bytes: &[u8]| CString::new(bytes).map_err(|_| refuse(FORMAT, "embedded NUL"));
        roots.push(Root {
            name: c_string(name)?,
            source: c_string(source)?,
            patched: c_string(patched)?,
            tag: c_string(optional(b"tag")?)?,
            // Widen the first byte as a signed char to preserve the v1 ABI.
            stage: stage as i8 as u32,
        });
    }
    Ok(roots)
}

struct Session {
    pack: Pack,
    hash: CString,
    roots: Vec<Root>,
    // Empty when the host supplied no cache directory.
    cache: PathBuf,
    compiler: *mut c_void,
    compiler_hash: String,
    claims: Vec<Vec<u8>>,
    // Serializes prepare and holds the last variant until the next prepare or close.
    output: Mutex<Vec<u8>>,
}

fn compiler_identity(compiler: *mut c_void) -> Outcome<String> {
    let mut module: HMODULE = ptr::null_mut();
    let flags =
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    let found = unsafe { GetModuleHandleExW(flags, compiler.cast(), &mut module) } != 0;
    check(found, ARGUMENT, "compiler module")?;
    let mut path = vec![0u16; 32768];
    let length = unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32) };
    check(length != 0, ARGUMENT, "compiler path")?;
    path.truncate((length as usize).min(path.len()));
    let image = fs::read(PathBuf::from(std::ffi::OsString::from_wide(&path))).unwrap_or_default();
    check(!image.is_empty(), ARGUMENT, "compiler identity unreadable")?;
    sha256(&image)
}

fn open_session(
    platform: &Platform,
    blob: rsf_patch_bytes,
    cache: *const u16,
    compiler: *mut c_void,
) -> Outcome<Session> {
    let pack = Pack::load(platform, bytes(blob)?)?;
    let cache = if cache.is_null() {
        PathBuf::new()
    } else {
        unsafe { wide_path(cache) }
    };
    let roots = admit_roots(&pack)?;
    let mut claims: Vec<_> = roots
        .iter()
        .map(|root| root.source.to_bytes().to_vec())
        .collect();
    claims.sort_unstable();
    claims.dedup();
    let compiler_hash = compiler_identity(compiler)?;
    claim(&pack.id, &claims)?;
    let hash = CString::new(pack.hash.clone()).unwrap_or_default();
    Ok(Session {
        pack,
        hash,
        roots,
        cache,
        compiler,
        compiler_hash,
        claims,
        output: Mutex::new(Vec::new()),
    })
}

unsafe extern "C" fn open(
    blob: rsf_patch_bytes,
    cache: *const u16,
    compiler: *mut c_void,
    out: *mut *mut rsf_patch_session,
) -> i32 {
    if !out.is_null() {
        unsafe { *out = ptr::null_mut() };
    }
    guarded(|| {
        if out.is_null() || compiler.is_null() {
            return ARGUMENT;
        }
        let session = match open_session(platform(), blob, cache, compiler) {
            Ok(session) => session,
            Err(refusal) => return refusal.code,
        };
        let claims = session.claims.clone();
        let handle = crate::allocate_handle(session);
        if handle.is_null() {
            unclaim(&claims);
            return INTERNAL;
        }
        unsafe { *out = handle.cast() };
        OK
    })
}

unsafe fn session<'a>(s: *const rsf_patch_session) -> Option<&'a Session> {
    unsafe { s.cast::<Session>().as_ref() }
}

unsafe extern "C" fn hash(s: *const rsf_patch_session) -> *const c_char {
    unsafe { session(s) }.map_or(ptr::null(), |s| s.hash.as_ptr())
}

unsafe extern "C" fn root_count(s: *const rsf_patch_session) -> u32 {
    unsafe { session(s) }.map_or(0, |s| s.roots.len() as u32)
}

unsafe extern "C" fn root(
    s: *const rsf_patch_session,
    index: u32,
    out: *mut rsf_patch_root,
) -> i32 {
    let Some(s) = (unsafe { session(s) }) else {
        return ARGUMENT;
    };
    let out = match unsafe { record(out) } {
        Ok(out) => out,
        Err(code) => return code,
    };
    match s.roots.get(index as usize) {
        None => ARGUMENT,
        Some(root) => {
            *out = rsf_patch_root {
                struct_size: size_of::<rsf_patch_root>() as u32,
                abi_version: VERSION,
                name: root.name.as_ptr(),
                source_sha256: root.source.as_ptr(),
                patched_sha256: root.patched.as_ptr(),
                tag: root.tag.as_ptr(),
                stage: root.stage,
            };
            OK
        }
    }
}

struct Compilation<'a> {
    files: &'a Files,
    code: Option<Vec<u8>>,
    errors: Option<Vec<u8>>,
}

fn compilation<'a, 'b>(user: *mut c_void) -> &'a mut Compilation<'b> {
    unsafe { &mut *user.cast::<Compilation>() }
}

/// D3DCompile resolves includes case-insensitively with either separator.
unsafe fn include(
    user: *mut c_void,
    name: *const c_char,
    data: *mut *const c_void,
    size: *mut usize,
) -> i32 {
    let mut key = unsafe { c_bytes(name) }.to_vec();
    for c in &mut key {
        *c = if *c == b'\\' {
            b'/'
        } else {
            c.to_ascii_lowercase()
        };
    }
    let Some(found) = compilation(user).files.get(&key) else {
        return -1;
    };
    unsafe {
        *data = found.as_ptr().cast();
        *size = found.len();
    }
    0
}

fn copy(data: *const c_void, size: usize) -> Vec<u8> {
    if size == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) }.to_vec()
    }
}

unsafe fn code(user: *mut c_void, data: *const c_void, size: usize) {
    compilation(user).code = Some(copy(data, size));
}

unsafe fn errors(user: *mut c_void, data: *const c_void, size: usize) {
    compilation(user).errors = Some(copy(data, size));
}

/// Compiles `source` with the caller's inputs and includes from `files`.
fn compile(
    platform: &Platform,
    compiler: *mut c_void,
    r: &rsf_patch_request,
    files: &Files,
    source: &[u8],
) -> Outcome<Vec<u8>> {
    if r.define_count != 0 {
        let defines = unsafe { std::slice::from_raw_parts(r.defines, r.define_count) };
        let complete = defines
            .iter()
            .all(|d| !d.name.is_null() && !d.value.is_null());
        check(complete, ARGUMENT, "null define")?;
    }
    let mut state = Compilation {
        files,
        code: None,
        errors: None,
    };
    let call = CompileCall {
        compiler,
        source: raw(source),
        defines: r.defines,
        define_count: r.define_count,
        entry: r.entry,
        target: r.target,
        flags: r.flags,
        user: (&raw mut state).cast(),
        include: Some(include),
        code: Some(code),
        errors: Some(errors),
    };
    let result = unsafe { (platform.compile)(&call) };
    match state.code {
        Some(code) if result >= 0 => Ok(code),
        _ => Err(Refusal {
            code: COMPILE,
            reason: match state.errors {
                Some(mut text) => {
                    text.truncate(COMPILE_ERROR_LIMIT);
                    Cow::Owned(text)
                }
                None => Cow::Borrowed(b"D3DCompile failed"),
            },
        }),
    }
}

/// Acquires, proves, builds or reuses one variant. Returns whether the cache supplied it.
fn prepare_variant(
    platform: &Platform,
    s: &Session,
    r: &rsf_patch_request,
    output: &mut Vec<u8>,
) -> Outcome<bool> {
    let rules = &s.pack.rules;
    let Some(Json::Obj(sources)) = rules.get(b"sources") else {
        return Err(refuse(FORMAT, "source list"));
    };
    let resolve = r.resolve.expect("checked by the caller");
    let mut originals = Files::new();
    for (name, _) in sources {
        let c_name = CString::new(name.as_slice()).unwrap_or_default();
        let mut provided = rsf_patch_bytes::default();
        let status = unsafe { resolve(r.user, c_name.as_ptr(), &mut provided) };
        check(status == 0, SOURCE, "engine source provider")?;
        let content = bytes(provided)?.to_vec();
        originals.entry(name.clone()).or_insert(content);
    }
    let patched = s.pack.transform(&originals)?;
    let source = bytes(r.source)?;
    let stock = bytes(r.original_bytecode)?;
    let mut root_name: Option<&[u8]> = None;
    for candidate in &s.roots {
        let name = candidate.name.to_bytes();
        if originals
            .get(name)
            .is_some_and(|original| original == source)
        {
            check(root_name.is_none(), HASH, "ambiguous root")?;
            root_name = Some(name);
            let expected = candidate.patched.to_bytes();
            if !expected.is_empty() {
                let matches = sha256(&patched[name])?.as_bytes() == expected;
                check(matches, HASH, "patched root hash")?;
            }
        }
    }
    let root_name =
        root_name.ok_or_else(|| refuse(HASH, "root not allowlisted/acquired from engine"))?;
    let proof = compile(platform, s.compiler, r, &originals, source)?;
    check(
        proof == stock,
        ORIGINAL,
        "original does not reproduce engine DXBC",
    )?;
    let mut key = format!(
        "{CACHE_PREFIX}{}{}{}",
        s.pack.hash,
        s.compiler_hash,
        sha256(stock)?
    )
    .into_bytes();
    let mut field = |value: &[u8]| {
        key.extend_from_slice(format!("{}:", value.len()).as_bytes());
        key.extend_from_slice(value);
    };
    field(root_name);
    field(sha256(source)?.as_bytes());
    field(unsafe { c_bytes(r.entry) });
    field(unsafe { c_bytes(r.target) });
    field(r.flags.to_string().as_bytes());
    if r.define_count != 0 {
        for d in unsafe { std::slice::from_raw_parts(r.defines, r.define_count) } {
            field(unsafe { c_bytes(d.name) });
            field(unsafe { c_bytes(d.value) });
        }
    }
    let cached = !s.cache.as_os_str().is_empty();
    let path = s.cache.join(format!("{}.dxbc", sha256(&key)?));
    *output = if cached {
        fs::read(&path).unwrap_or_default()
    } else {
        Vec::new()
    };
    if !output.is_empty() {
        if reflection::admit(platform, rules, stock, output).is_ok() {
            COUNTERS.cache_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(true);
        }
        output.clear();
    }
    *output = compile(platform, s.compiler, r, &patched, &patched[root_name])?;
    reflection::admit(platform, rules, stock, output)?;
    COUNTERS.built.fetch_add(1, Ordering::Relaxed);
    if cached {
        // Publish only a completely written file, through a name no other writer uses.
        let _ = fs::create_dir_all(&s.cache);
        let mut temporary = path.clone().into_os_string();
        let writer = TEMPORARY.fetch_add(1, Ordering::Relaxed);
        temporary.push(format!(".{}.{writer}.tmp", std::process::id()));
        if fs::write(&temporary, &output[..]).is_err() || fs::rename(&temporary, &path).is_err() {
            let _ = fs::remove_file(&temporary);
        }
    }
    Ok(false)
}

fn count_refusal(code: i32) {
    COUNTERS.refused.fetch_add(1, Ordering::Relaxed);
    if (INTERNAL..0).contains(&code) {
        COUNTERS.refusals[code.unsigned_abs() as usize].fetch_add(1, Ordering::Relaxed);
    }
}

unsafe extern "C" fn prepare(
    s: *mut rsf_patch_session,
    r: *const rsf_patch_request,
    out: *mut rsf_patch_output,
) -> i32 {
    let out = match unsafe { record(out) } {
        Ok(out) => out,
        Err(code) => return code,
    };
    out.bytecode = rsf_patch_bytes::default();
    out.cache_hit = 0;
    out.reason[0] = 0;
    let result = catch_unwind(AssertUnwindSafe(|| -> Outcome<()> {
        let request = unsafe { has_capacity(r) }.then(|| unsafe { &*r });
        let (Some(s), Some(r)) = (unsafe { session(s) }, request) else {
            return Err(refuse(ARGUMENT, "request size"));
        };
        check(r.abi_version == VERSION, ABI, "request version")?;
        let inputs = !r.entry.is_null()
            && !r.target.is_null()
            && r.resolve.is_some()
            && (r.define_count == 0 || !r.defines.is_null());
        check(inputs, ARGUMENT, "compile inputs")?;
        let mut output = lock(&s.output);
        let hit = prepare_variant(platform(), s, r, &mut output)?;
        out.bytecode = raw(&output);
        out.cache_hit = u32::from(hit);
        Ok(())
    }));
    let refusal = match result {
        Ok(Ok(())) => return OK,
        Ok(Err(refusal)) => refusal,
        Err(_) => refuse(INTERNAL, "shader patch exception"),
    };
    count_refusal(refusal.code);
    write_reason(&mut out.reason, &refusal.reason);
    refusal.code
}

unsafe extern "C" fn close(s: *mut rsf_patch_session) {
    if s.is_null() {
        return;
    }
    let session = unsafe { Box::from_raw(s.cast::<Session>()) };
    unclaim(&session.claims);
}

unsafe extern "C" fn counters(out: *mut rsf_patch_counters) -> i32 {
    let out = match unsafe { record(out) } {
        Ok(out) => out,
        Err(code) => return code,
    };
    *out = rsf_patch_counters {
        struct_size: size_of::<rsf_patch_counters>() as u32,
        abi_version: VERSION,
        intercepted: COUNTERS.intercepted.load(Ordering::Relaxed),
        built: COUNTERS.built.load(Ordering::Relaxed),
        cache_hits: COUNTERS.cache_hits.load(Ordering::Relaxed),
        refused: COUNTERS.refused.load(Ordering::Relaxed),
        refusals: std::array::from_fn(|i| COUNTERS.refusals[i].load(Ordering::Relaxed)),
    };
    OK
}

/// Fills the v1 table. Every client shares this owner's sessions, claims, hooks and counters.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_get_shader_patch_api(api: *mut rsf_shader_patch_api) -> i32 {
    let api = match unsafe { record(api) } {
        Ok(api) => api,
        Err(code) => return code,
    };
    *api = rsf_shader_patch_api {
        struct_size: size_of::<rsf_shader_patch_api>() as u32,
        abi_version: VERSION,
        open: Some(open),
        hash: Some(hash),
        root_count: Some(root_count),
        root: Some(root),
        prepare: Some(prepare),
        close: Some(close),
        attach_d3d11: Some(d3d11::attach),
        create_d3d11: Some(d3d11::create_shader),
        constant_buffer: Some(d3d11::constant_buffer),
        upload_constant: Some(d3d11::upload_constant),
        bind_constant: Some(d3d11::bind_constant),
        counters: Some(counters),
        dump: Some(d3d11::dump),
    };
    OK
}
