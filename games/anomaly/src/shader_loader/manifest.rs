// SPDX-License-Identifier: MIT
use super::Bytes;
use std::collections::HashSet;
use std::ffi::{CStr, c_char, c_void};

const FAMILIES: [(&CStr, u8); 8] = [
    (c"none", 0),
    (c"core_static", b'v'),
    (c"core_model", b'v'),
    (c"core_pixel", b'p'),
    (c"foliage_tree", b'v'),
    (c"foliage_detail", b'v'),
    (c"tess_hull", b'h'),
    (c"tess_domain", b'd'),
];

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_shader_family_name(family: u8) -> *const c_char {
    FAMILIES
        .get(family as usize)
        .map_or(c"unknown", |f| f.0)
        .as_ptr()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_shader_family_stage(family: u8) -> u8 {
    FAMILIES.get(family as usize).map_or(0, |f| f.1)
}

#[repr(C)]
pub(super) struct Root {
    pub name: Bytes,
    pub original: Bytes,
    pub motion: Bytes,
    tag: Bytes,
    pub stage: u32,
}

#[repr(C)]
struct ManifestSink {
    user: *mut c_void,
    root: unsafe extern "C" fn(*mut c_void, &Root, u8) -> bool,
    finish: unsafe extern "C" fn(*mut c_void, Bytes, bool) -> bool,
}

type ReadRoot = unsafe extern "C" fn(*const c_void, u32, *mut Root) -> bool;

fn hash_text(text: &[u8]) -> bool {
    text.len() == 64 && text.iter().all(|&b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn safe_name(name: &[u8]) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.windows(2).any(|pair| pair == b"..")
        && !name.starts_with(b"/")
        && !name.ends_with(b"/")
        && name.iter().filter(|&&b| b == b'/').count() <= 1
        && name
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"_-./".contains(b))
}

#[derive(Clone, Copy)]
enum Rejection {
    Invalid,
    Duplicate,
    Sink,
}

unsafe fn add_root(
    sink: &ManifestSink,
    sources: &mut HashSet<&[u8]>,
    root: &Root,
    from_tree: bool,
) -> Result<(), Rejection> {
    // Both callers keep metadata alive until admission finishes.
    let original = unsafe { root.original.as_slice() };
    let motion = unsafe { root.motion.as_slice() };
    let name = unsafe { root.name.as_slice() };
    let tag = unsafe { root.tag.as_slice() };
    let family = FAMILIES.iter().position(|&(label, stage)| {
        stage != 0 && u32::from(stage) == root.stage && tag == label.to_bytes()
    });
    // Only the fixture opens the root path on disk. Runtime names are engine filesystem keys.
    let Some(family) = family.filter(|_| {
        !name.is_empty()
            && (!from_tree || safe_name(name))
            && hash_text(original)
            && hash_text(motion)
    }) else {
        return Err(Rejection::Invalid);
    };
    if !sources.insert(original) {
        return Err(Rejection::Duplicate);
    }
    if !unsafe { (sink.root)(sink.user, root, family as u8) } {
        return Err(Rejection::Sink);
    }
    Ok(())
}

impl ManifestSink {
    fn finish(&self, result: Result<&[u8], String>) -> bool {
        match result {
            Ok(namespace) => unsafe { (self.finish)(self.user, Bytes::from(namespace), true) },
            Err(error) => {
                unsafe { (self.finish)(self.user, Bytes::from(error.as_bytes()), false) };
                false
            }
        }
    }
}

unsafe fn parse<'a>(text: &'a [u8], sink: &ManifestSink) -> Result<&'a [u8], String> {
    let mut namespace: &[u8] = &[];
    let mut sources = HashSet::new();
    let mut header = false;
    for (i, line) in text.split(|&b| b == b'\n').enumerate() {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() || line.starts_with(b"#") {
            continue;
        }
        let number = i + 1;
        let mut words = line
            .split(|&b| matches!(b, b'\t'..=b'\r' | b' '))
            .filter(|word| !word.is_empty());
        let kind = words.next().unwrap_or_default();
        if !header {
            if kind != b"rsf-anomaly-motion-manifest" || words.next() != Some(b"1") {
                return Err("bad manifest header".into());
            }
            header = true;
            continue;
        }
        if kind == b"namespace" {
            namespace = words.next().unwrap_or_default();
            if !hash_text(namespace) {
                return Err(format!("bad namespace at line {number}"));
            }
            continue;
        }
        if kind != b"root" {
            return Err(format!("unknown record at line {number}"));
        }
        let stage = match words.next() {
            Some(b"vs") => b'v',
            Some(b"ps") => b'p',
            Some(b"hs") => b'h',
            Some(b"ds") => b'd',
            _ => return Err(format!("bad root record at line {number}")),
        };
        let [Some(tag), Some(name), Some(original), Some(motion), None] =
            std::array::from_fn(|_| words.next())
        else {
            return Err(format!("bad root record at line {number}"));
        };
        let root = Root {
            name: Bytes::from(name),
            original: Bytes::from(original),
            motion: Bytes::from(motion),
            tag: Bytes::from(tag),
            stage: u32::from(stage),
        };
        match unsafe { add_root(sink, &mut sources, &root, true) } {
            Ok(()) => {}
            Err(Rejection::Invalid) => return Err(format!("bad root record at line {number}")),
            Err(Rejection::Duplicate) => {
                return Err(format!("duplicate original source at line {number}"));
            }
            Err(Rejection::Sink) => return Err("manifest storage failed".into()),
        }
    }
    if !header || namespace.is_empty() || sources.is_empty() {
        return Err("incomplete manifest".into());
    }
    Ok(namespace)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_manifest(text: Bytes, sink: &ManifestSink) -> bool {
    sink.finish(unsafe { parse(text.as_slice(), sink) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_pack_manifest(
    namespace: Bytes,
    count: u32,
    session: *const c_void,
    read: ReadRoot,
    sink: &ManifestSink,
) -> bool {
    let namespace = unsafe { namespace.as_slice() };
    let admit = || {
        if count == 0 || !hash_text(namespace) {
            return Err("incomplete pack manifest".into());
        }
        let mut sources = HashSet::new();
        for i in 0..count {
            let mut root = std::mem::MaybeUninit::uninit();
            if !unsafe { read(session, i, root.as_mut_ptr()) } {
                return Err("pack root metadata".into());
            }
            match unsafe { add_root(sink, &mut sources, &root.assume_init(), false) } {
                Ok(()) => {}
                Err(Rejection::Invalid) => return Err("invalid adapter root metadata".into()),
                Err(Rejection::Duplicate) => return Err("ambiguous root source".into()),
                Err(Rejection::Sink) => return Err("manifest storage failed".into()),
            }
        }
        Ok(namespace)
    };
    sink.finish(admit())
}
