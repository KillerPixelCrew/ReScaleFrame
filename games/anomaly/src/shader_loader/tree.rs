// SPDX-License-Identifier: MIT
//! Deployed-tree preparation: the stock permutation is compiled from the original tree and the
//! variant from the motion root, with a DXBC cache. Only the motion fixtures prepare from a tree;
//! the game prepares variants from the runtime pack.
use super::reflection::{Compatibility, Reflection, rsf_anomaly_shader_compatible};
use super::{Bytes, Entry, Manifest, Outcome, Request};
use rsf_windows::sha256_hex;
use std::ffi::CStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

#[repr(C)]
pub(super) struct Identity {
    pub(super) namespace: Bytes,
    pub(super) original: Bytes,
    pub(super) motion: Bytes,
    pub(super) name: Bytes,
    pub(super) stage: u8,
}

pub(super) unsafe fn rsf_anomaly_shader_match_request(
    manifest: Option<&Manifest>,
    source: Bytes,
    target: Bytes,
) -> Option<&Entry> {
    let hash = sha256_hex(unsafe { source.as_slice() })?;
    let entry = manifest?.find(&hash)?;
    (unsafe { target.as_slice() }.first() == Some(&entry.stage)).then_some(entry)
}

pub(super) fn cache_key(request: &Request, id: &Identity) -> Option<[u8; 64]> {
    let stock_hash = sha256_hex(unsafe { request.bytecode.as_slice() })?;
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
    sha256_hex(&key)
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

pub(super) struct TreeResult<R> {
    pub(super) outcome: Outcome,
    pub(super) compatibility: Compatibility,
    pub(super) stock: Option<R>,
    pub(super) reflection: Option<R>,
    pub(super) bytes: Vec<u8>,
    pub(super) detail: Vec<u8>,
}

impl<R> TreeResult<R> {
    fn refused(outcome: Outcome, detail: impl AsRef<[u8]>) -> Self {
        Self {
            outcome,
            compatibility: Compatibility::Ok,
            stock: None,
            reflection: None,
            bytes: Vec::new(),
            detail: detail.as_ref().to_vec(),
        }
    }
}

pub(super) fn prepare_tree<R>(
    request: &Request,
    id: &Identity,
    deployment: &Path,
    cache: Option<&Path>,
    mut reflect: impl FnMut(bool, &[u8]) -> Option<R>,
    view: impl Fn(&R) -> &Reflection,
    mut compile: impl FnMut(bool, &[u8]) -> Result<Vec<u8>, Vec<u8>>,
) -> TreeResult<R> {
    let bytecode = unsafe { request.bytecode.as_slice() };
    let Some(stock) = reflect(true, bytecode) else {
        return TreeResult::refused(
            Outcome::ReflectionMismatch,
            b"stock bytecode does not reflect",
        );
    };
    let cache_path = cache.and_then(|path| {
        let key = cache_key(request, id)?;
        Some(path.join(format!("{}.dxbc", std::str::from_utf8(&key).unwrap())))
    });
    let check = |variant: &R| rsf_anomaly_shader_compatible(id.stage, view(&stock), view(variant));
    if let Some(path) = &cache_path
        && path.is_file()
        && let Ok(bytes) = fs::read(path)
        && !bytes.is_empty()
        && let Some(reflection) = reflect(false, &bytes)
        && check(&reflection) == Compatibility::Ok
    {
        return TreeResult {
            outcome: Outcome::ReplacedFromCache,
            compatibility: Compatibility::Ok,
            stock: Some(stock),
            reflection: Some(reflection),
            bytes,
            detail: Vec::new(),
        };
    }

    let mut prepare = || {
        let compiled = compile(false, unsafe { request.source.as_slice() })
            .map_err(|error| TreeResult::refused(Outcome::OriginalCompileFailed, error))?;
        if compiled != bytecode {
            return Err(TreeResult::refused(
                Outcome::OriginalMismatch,
                b"deployed original tree does not reproduce the engine bytecode",
            ));
        }
        let mut root_path = deployment.join("motion").join("r3");
        // Build individual path components so extended-length Windows paths keep native separators.
        let root = std::str::from_utf8(unsafe { id.name.as_slice() })
            .ok()
            .and_then(|name| {
                root_path.extend(name.split('/'));
                fs::read(root_path).ok()
            })
            .filter(|bytes| {
                sha256_hex(bytes).is_some_and(|hash| hash.as_slice() == unsafe { id.motion.as_slice() })
            })
            .ok_or_else(|| {
                TreeResult::refused(
                    Outcome::MotionCompileFailed,
                    b"motion root missing or not the manifest's",
                )
            })?;
        let bytes = compile(true, &root)
            .map_err(|error| TreeResult::refused(Outcome::MotionCompileFailed, error))?;
        let reflection = reflect(false, &bytes).ok_or_else(|| {
            TreeResult::refused(Outcome::ReflectionMismatch, b"variant does not reflect")
        })?;
        let compatibility = check(&reflection);
        if compatibility != Compatibility::Ok {
            return Err(TreeResult {
                compatibility,
                ..TreeResult::refused(Outcome::ReflectionMismatch, compatibility.name())
            });
        }
        if let Some(path) = &cache_path {
            write_cache(path, &bytes);
        }
        Ok(TreeResult {
            outcome: Outcome::Replaced,
            compatibility,
            stock: None,
            reflection: Some(reflection),
            bytes,
            detail: Vec::new(),
        })
    };
    let mut result = match prepare() {
        Ok(result) | Err(result) => result,
    };
    result.stock = Some(stock);
    result
}

const COMPATIBILITY_NAMES: [&CStr; 8] = [
    c"ok",
    c"version",
    c"missing_binding",
    c"extra_binding",
    c"buffer",
    c"inputs",
    c"outputs",
    c"patch_constants",
];

impl Compatibility {
    pub(super) fn name(self) -> &'static [u8] {
        COMPATIBILITY_NAMES[self as usize].to_bytes()
    }
}
