// SPDX-License-Identifier: MIT
mod d3d;
mod frame;
mod manifest;
mod reflection;
mod runtime_pack;

use crate::digest::sha256;
pub(crate) use frame::{DecodedFrame, FrameLayout, Read, decode_frame};
pub(crate) use manifest::{Entry, Manifest};
use reflection::{Compatibility, Reflection, rsf_anomaly_shader_compatible};
pub(crate) use runtime_pack::RuntimePack;
use std::ffi::{OsString, c_void};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct Span<T> {
    data: *const T,
    len: usize,
}

impl<T> From<&[T]> for Span<T> {
    fn from(items: &[T]) -> Self {
        Self {
            data: items.as_ptr(),
            len: items.len(),
        }
    }
}

impl<T> Span<T> {
    // Borrow only while the native owner (or the synchronous Rust caller) keeps it alive.
    pub(crate) unsafe fn as_slice<'a>(&self) -> &'a [T] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.data, self.len) }
        }
    }
}

pub(crate) type Bytes = Span<u8>;

#[derive(Clone, Copy)]
#[repr(C)]
struct Define {
    name: Bytes,
    value: Bytes,
}

#[repr(C)]
pub(crate) struct Request {
    target: Bytes,
    entry: Bytes,
    flags: u32,
    defines: Span<Define>,
    source: Bytes,
    bytecode: Bytes,
}

#[repr(C)]
struct Identity {
    namespace: Bytes,
    original: Bytes,
    motion: Bytes,
    name: Bytes,
    stage: u8,
}

const _: () = {
    assert!(size_of::<Request>() == 88);
    assert!(std::mem::offset_of!(Request, defines) == 40);
    assert!(std::mem::offset_of!(Request, bytecode) == 72);
};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_sha256(bytes: Bytes, out: &mut [u8; 64]) -> bool {
    let Some(hash) = sha256(unsafe { bytes.as_slice() }) else {
        return false;
    };
    *out = hash;
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_stage_matches(target: Bytes, stage: u8) -> bool {
    unsafe { target.as_slice() }.first() == Some(&stage)
}

fn cache_key(request: &Request, id: &Identity) -> Option<[u8; 64]> {
    let stock_hash = sha256(unsafe { request.bytecode.as_slice() })?;
    let flags = request.flags.to_string();
    let mut key = b"rsf-anomaly-motion-variant 1\n".to_vec();
    for field in [
        id.namespace,
        id.original,
        id.motion,
        request.target,
        request.entry,
        Bytes::from(flags.as_bytes()),
        Bytes::from(stock_hash.as_slice()),
    ] {
        key.extend_from_slice(unsafe { field.as_slice() });
        key.push(b'\n');
    }
    for define in unsafe { request.defines.as_slice() } {
        key.extend_from_slice(unsafe { define.name.as_slice() });
        key.push(b'=');
        key.extend_from_slice(unsafe { define.value.as_slice() });
        key.push(b'\n');
    }
    sha256(&key)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_shader_cache_key(
    request: &Request,
    id: &Identity,
    out: &mut [u8; 64],
) -> bool {
    let Some(hash) = cache_key(request, id) else {
        return false;
    };
    *out = hash;
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_skin_mode(defines: Span<Define>) -> i32 {
    unsafe { defines.as_slice() }
        .iter()
        .fold(-2, |mode, define| match unsafe { define.name.as_slice() } {
            b"SKIN_NONE" => -1,
            [b'S', b'K', b'I', b'N', b'_', digit @ b'0'..=b'4'] => i32::from(digit - b'0'),
            _ => mode,
        })
}

#[repr(u8)]
#[derive(Clone, Copy)]
enum Outcome {
    Replaced = 1,
    ReplacedFromCache = 2,
    OriginalCompileFailed = 5,
    OriginalMismatch = 6,
    MotionCompileFailed = 7,
    ReflectionMismatch = 8,
}

type Reflect = unsafe extern "C" fn(*mut c_void, bool, Bytes, *mut Reflection) -> bool;
type Complete = unsafe extern "C" fn(*mut c_void, Outcome, Compatibility, i32, Bytes, Bytes);
type Compile = unsafe extern "C" fn(*mut c_void, bool, Bytes, *mut Bytes) -> bool;

#[repr(C)]
struct Preparation {
    user: *mut c_void,
    reflect: Reflect,
    complete: Complete,
}

impl Preparation {
    fn reflect(&self, stock: bool, bytes: &[u8]) -> Option<Reflection> {
        let mut view = std::mem::MaybeUninit::uninit();
        unsafe {
            (self.reflect)(self.user, stock, Bytes::from(bytes), view.as_mut_ptr())
                .then(|| view.assume_init())
        }
    }

    fn check(
        &self,
        stage: u8,
        stock: &Reflection,
        bytes: &[u8],
    ) -> Result<(), (Compatibility, &'static [u8])> {
        let variant = self
            .reflect(false, bytes)
            .ok_or((Compatibility::Ok, b"variant does not reflect".as_slice()))?;
        let check = rsf_anomaly_shader_compatible(stage, stock, &variant);
        if check == Compatibility::Ok {
            Ok(())
        } else {
            Err((check, check.name()))
        }
    }
}

struct ResultSink<'a> {
    host: &'a Preparation,
    skin: i32,
}

impl ResultSink<'_> {
    fn complete(
        &self,
        outcome: Outcome,
        compatibility: Compatibility,
        detail: &[u8],
        variant: &[u8],
    ) {
        unsafe {
            (self.host.complete)(
                self.host.user,
                outcome,
                compatibility,
                self.skin,
                Bytes::from(detail),
                Bytes::from(variant),
            )
        };
    }

    fn refuse(&self, outcome: Outcome, detail: &[u8]) {
        self.complete(outcome, Compatibility::Ok, detail, &[]);
    }
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn write_cache(path: &Path, bytes: &[u8]) {
    if let Some(directory) = path.parent() {
        let _ = fs::create_dir_all(directory);
    }
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = path.with_extension(format!("dxbc.{}.{sequence}.tmp", std::process::id()));
    // A failed or concurrent writer must never publish partial DXBC under the cache key.
    if let Ok(mut file) = OpenOptions::new().write(true).create_new(true).open(&temp) {
        let written = file.write_all(bytes).is_ok();
        drop(file);
        if !written || fs::rename(&temp, path).is_err() {
            let _ = fs::remove_file(temp);
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_prepare_tree(
    request: &Request,
    id: &Identity,
    deployment: Span<u16>,
    cache: Span<u16>,
    host: &Preparation,
    compile: Compile,
) {
    let sink = ResultSink {
        host,
        skin: unsafe { rsf_anomaly_shader_skin_mode(request.defines) },
    };
    let bytecode = unsafe { request.bytecode.as_slice() };
    let Some(stock) = host.reflect(true, bytecode) else {
        return sink.refuse(
            Outcome::ReflectionMismatch,
            b"stock bytecode does not reflect",
        );
    };
    let cache = unsafe { cache.as_slice() };
    let cache_path = (!cache.is_empty())
        .then(|| cache_key(request, id))
        .flatten()
        .map(|key| {
            let mut path = PathBuf::from(OsString::from_wide(cache));
            path.push(format!("{}.dxbc", std::str::from_utf8(&key).unwrap()));
            path
        });
    if let Some(path) = &cache_path
        && path.is_file()
        && let Ok(bytes) = fs::read(path)
        && !bytes.is_empty()
        && host.check(id.stage, &stock, &bytes).is_ok()
    {
        return sink.complete(Outcome::ReplacedFromCache, Compatibility::Ok, &[], &bytes);
    }
    let mut compiled = Bytes::from(&[][..]);
    if !unsafe { compile(host.user, false, request.source, &mut compiled) } {
        return sink.refuse(Outcome::OriginalCompileFailed, unsafe {
            compiled.as_slice()
        });
    }
    if unsafe { compiled.as_slice() } != bytecode {
        return sink.refuse(
            Outcome::OriginalMismatch,
            b"deployed original tree does not reproduce the engine bytecode",
        );
    }
    let mut root_path = PathBuf::from(OsString::from_wide(unsafe { deployment.as_slice() }));
    root_path.push("motion");
    root_path.push("r3");
    // Build individual path components so extended-length Windows paths keep native separators.
    let name = unsafe { id.name.as_slice() };
    let root = std::str::from_utf8(name)
        .ok()
        .and_then(|name| {
            root_path.extend(name.split('/'));
            fs::read(root_path).ok()
        })
        .filter(|bytes| {
            sha256(bytes).is_some_and(|hash| hash.as_slice() == unsafe { id.motion.as_slice() })
        });
    let Some(root) = root else {
        return sink.refuse(
            Outcome::MotionCompileFailed,
            b"motion root missing or not the manifest's",
        );
    };
    if !unsafe { compile(host.user, true, Bytes::from(root.as_slice()), &mut compiled) } {
        return sink.refuse(Outcome::MotionCompileFailed, unsafe { compiled.as_slice() });
    }
    let variant = unsafe { compiled.as_slice() };
    if let Err((compatibility, detail)) = host.check(id.stage, &stock, variant) {
        return sink.complete(Outcome::ReflectionMismatch, compatibility, detail, &[]);
    }
    if let Some(path) = &cache_path {
        write_cache(path, variant);
    }
    sink.complete(Outcome::Replaced, Compatibility::Ok, &[], variant);
}

// The generic Runtime v1 service owns the original DXBC proof and production cache.
// Adapter authorization additionally requires its own stage reflection policy.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_finish_runtime(
    request: &Request,
    stage: u8,
    status: i32,
    cache_hit: bool,
    variant: Bytes,
    detail: Bytes,
    host: &Preparation,
) {
    let sink = ResultSink {
        host,
        skin: unsafe { rsf_anomaly_shader_skin_mode(request.defines) },
    };
    let detail = unsafe { detail.as_slice() };
    let refusal = match status {
        0 => None,
        -7 => Some(Outcome::OriginalMismatch),
        -8 => Some(Outcome::ReflectionMismatch),
        _ => Some(Outcome::MotionCompileFailed),
    };
    if let Some(outcome) = refusal {
        return sink.refuse(outcome, detail);
    }
    let Some(stock) = host.reflect(true, unsafe { request.bytecode.as_slice() }) else {
        return sink.refuse(Outcome::ReflectionMismatch, detail);
    };
    let variant = unsafe { variant.as_slice() };
    if let Err((compatibility, _)) = host.check(stage, &stock, variant) {
        return sink.complete(Outcome::ReflectionMismatch, compatibility, detail, &[]);
    }
    let outcome = if cache_hit {
        Outcome::ReplacedFromCache
    } else {
        Outcome::Replaced
    };
    sink.complete(outcome, Compatibility::Ok, detail, variant);
}
