// SPDX-License-Identifier: MIT
//! The admitted motion manifest. Rust owns its rows and the original-source digest index for
//! both the fixture text form and the Runtime pack form. Native code holds the opaque object.
use super::{Bytes, Identity};
use crate::shader_family::FAMILIES;
use std::collections::HashSet;
use std::ffi::{CStr, c_char, c_void};

#[repr(C)]
pub(super) struct Root {
    pub name: Bytes,
    pub original: Bytes,
    pub motion: Bytes,
    pub(super) tag: Bytes,
    pub stage: u32,
}

pub(super) type ReadRoot = unsafe extern "C" fn(*const c_void, u32, *mut Root) -> bool;

/// One admitted root, laid out as `rsf_anomaly_shader_entry`. Its text borrows the owning
/// manifest's storage, so a row is only reachable through that manifest.
#[repr(C)]
pub(crate) struct Entry {
    name: Bytes,
    original: Bytes,
    motion: Bytes,
    pub stage: u8,
    pub family: u8,
}

const _: () = {
    assert!(size_of::<Entry>() == 56);
    assert!(std::mem::offset_of!(Entry, stage) == 48);
    assert!(std::mem::offset_of!(Entry, family) == 49);
};

impl Entry {
    pub fn name(&self) -> &[u8] {
        // The text lives in the manifest that lends `self`.
        unsafe { self.name.as_slice() }
    }

    pub fn original(&self) -> &[u8] {
        unsafe { self.original.as_slice() }
    }

    pub(super) fn identity(&self, namespace: &[u8]) -> Identity {
        Identity {
            namespace: Bytes::from(namespace),
            original: self.original,
            motion: self.motion,
            name: self.name,
            stage: self.stage,
        }
    }

    #[cfg(any(test, feature = "motion-binder-fixture"))]
    pub(super) fn motion(&self) -> &[u8] {
        unsafe { self.motion.as_slice() }
    }
}

/// Immutable after admission. Rows are ordered by original source SHA-256, which is also the
/// lookup index, so iteration is deterministic.
pub(crate) struct Manifest {
    namespace: Box<[u8]>,
    entries: Box<[Entry]>,
    _text: Vec<Box<[u8]>>,
}

// Nothing mutates a manifest after admission; rows only point into its own boxed text.
unsafe impl Send for Manifest {}
unsafe impl Sync for Manifest {}

impl Manifest {
    pub fn namespace(&self) -> &[u8] {
        &self.namespace
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn find(&self, original: &[u8]) -> Option<&Entry> {
        self.entries
            .binary_search_by(|entry| entry.original().cmp(original))
            .ok()
            .map(|i| &self.entries[i])
    }
}

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
}

struct Row<'a> {
    name: &'a [u8],
    original: &'a [u8],
    motion: &'a [u8],
    stage: u8,
    family: u8,
}

/// Validated rows of one candidate. They borrow the caller's metadata until `finish` copies
/// them, so a refused candidate never replaces an admitted manifest.
#[derive(Default)]
struct Candidate<'a> {
    sources: HashSet<&'a [u8]>,
    rows: Vec<Row<'a>>,
}

impl<'a> Candidate<'a> {
    /// Both callers keep `root`'s metadata alive until admission finishes.
    unsafe fn add(&mut self, root: &Root, from_tree: bool) -> Result<(), Rejection> {
        let original = unsafe { root.original.as_slice() };
        let motion = unsafe { root.motion.as_slice() };
        let name = unsafe { root.name.as_slice() };
        let tag = unsafe { root.tag.as_slice() };
        let family = FAMILIES.iter().find_map(|&(id, label, stage)| {
            (stage != 0 && u32::from(stage) == root.stage && tag == label.to_bytes()).then_some(id)
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
        if !self.sources.insert(original) {
            return Err(Rejection::Duplicate);
        }
        self.rows.push(Row {
            name,
            original,
            motion,
            stage: root.stage as u8,
            family,
        });
        Ok(())
    }

    fn finish(self, namespace: &[u8]) -> Manifest {
        let mut rows = self.rows;
        // Originals are unique, so this order is total.
        rows.sort_unstable_by(|a, b| a.original.cmp(b.original));
        let mut text = Vec::with_capacity(rows.len() * 3);
        let mut own = |bytes: &[u8]| {
            let owned: Box<[u8]> = bytes.into();
            // Moving the box into `text` keeps its heap storage in place.
            let view = Bytes::from(&*owned);
            text.push(owned);
            view
        };
        let entries = rows
            .iter()
            .map(|row| Entry {
                name: own(row.name),
                original: own(row.original),
                motion: own(row.motion),
                stage: row.stage,
                family: row.family,
            })
            .collect();
        Manifest {
            namespace: namespace.into(),
            entries,
            _text: text,
        }
    }
}

pub(super) fn parse(text: &[u8]) -> Result<Manifest, String> {
    let mut namespace: &[u8] = &[];
    let mut candidate = Candidate::default();
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
        match unsafe { candidate.add(&root, true) } {
            Ok(()) => {}
            Err(Rejection::Invalid) => return Err(format!("bad root record at line {number}")),
            Err(Rejection::Duplicate) => {
                return Err(format!("duplicate original source at line {number}"));
            }
        }
    }
    if !header || namespace.is_empty() || candidate.rows.is_empty() {
        return Err("incomplete manifest".into());
    }
    Ok(candidate.finish(namespace))
}

pub(super) unsafe fn admit_pack(
    namespace: &[u8],
    count: u32,
    session: *const c_void,
    read: ReadRoot,
) -> Result<Manifest, String> {
    if count == 0 || !hash_text(namespace) {
        return Err("incomplete pack manifest".into());
    }
    let mut candidate = Candidate::default();
    for i in 0..count {
        let mut root = std::mem::MaybeUninit::uninit();
        if !unsafe { read(session, i, root.as_mut_ptr()) } {
            return Err("pack root metadata".into());
        }
        match unsafe { candidate.add(&root.assume_init(), false) } {
            Ok(()) => {}
            Err(Rejection::Invalid) => return Err("invalid adapter root metadata".into()),
            Err(Rejection::Duplicate) => return Err("ambiguous root source".into()),
        }
    }
    Ok(candidate.finish(namespace))
}

#[repr(C)]
struct Refusal {
    user: *mut c_void,
    report: unsafe extern "C" fn(*mut c_void, Bytes),
}

/// Hands an admitted manifest to its native owner, or reports why the candidate was refused.
fn publish(result: Result<Manifest, String>, refusal: &Refusal) -> *mut Manifest {
    match result {
        Ok(manifest) => Box::into_raw(Box::new(manifest)),
        Err(error) => {
            unsafe { (refusal.report)(refusal.user, Bytes::from(error.as_bytes())) };
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_manifest(text: Bytes, refusal: &Refusal) -> *mut Manifest {
    publish(parse(unsafe { text.as_slice() }), refusal)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_admitted_free(manifest: *mut Manifest) {
    if !manifest.is_null() {
        drop(unsafe { Box::from_raw(manifest) });
    }
}

#[repr(C)]
struct View {
    namespace: Bytes,
    entries: *const Entry,
    count: usize,
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_shader_admitted_view(manifest: &Manifest, out: &mut View) {
    *out = View {
        namespace: Bytes::from(manifest.namespace()),
        entries: manifest.entries.as_ptr(),
        count: manifest.entries.len(),
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_admitted_find(
    manifest: &Manifest,
    original: Bytes,
) -> *const Entry {
    manifest
        .find(unsafe { original.as_slice() })
        .map_or(std::ptr::null(), std::ptr::from_ref)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn hash(digit: char) -> String {
        std::iter::repeat_n(digit, 64).collect()
    }

    fn text(roots: &[(&str, &str, &str, char, char)]) -> String {
        let mut out = format!("rsf-anomaly-motion-manifest 1\nnamespace {NS}\n");
        for (stage, tag, name, original, motion) in roots {
            out += &format!(
                "root {stage} {tag} {name} {} {}\n",
                hash(*original),
                hash(*motion)
            );
        }
        out
    }

    fn parsed(text: &str) -> Result<Manifest, String> {
        parse(text.as_bytes())
    }

    #[test]
    fn rows_are_owned_ordered_by_original_and_indexed_by_exact_digest() {
        let source = text(&[
            ("ps", "core_pixel", "b.ps", '9', '1'),
            ("vs", "core_static", "a.vs", '3', '2'),
            ("hs", "tess_hull", "dx11/tess.hs", 'c', '4'),
        ]);
        let manifest = parsed(&source).unwrap();
        drop(source);
        let order: Vec<_> = manifest.entries().iter().map(Entry::name).collect();
        assert_eq!(order, [&b"a.vs"[..], b"b.ps", b"dx11/tess.hs"]);
        assert_eq!(manifest.namespace(), NS.as_bytes());
        let hull = manifest.find(hash('c').as_bytes()).unwrap();
        assert_eq!((hull.stage, hull.family), (b'h', 6));
        assert_eq!(hull.motion(), hash('4').as_bytes());
        assert!(manifest.find(hash('4').as_bytes()).is_none());
        assert!(manifest.find(&hash('c').as_bytes()[..63]).is_none());
        assert!(manifest.find(hash('C').as_bytes()).is_none());
    }

    #[test]
    fn malformed_or_duplicate_text_is_refused() {
        assert_eq!(
            parsed("rsf-anomaly-motion-manifest 2\n").err().unwrap(),
            "bad manifest header"
        );
        assert_eq!(
            parsed(&text(&[("ps", "core_static", "x.ps", '1', '2')]))
                .err()
                .unwrap(),
            "bad root record at line 3"
        );
        assert_eq!(
            parsed(&text(&[
                ("vs", "core_static", "a.vs", '1', '2'),
                ("vs", "core_model", "b.vs", '1', '3'),
            ]))
            .err()
            .unwrap(),
            "duplicate original source at line 4"
        );
        assert_eq!(parsed(&text(&[])).err().unwrap(), "incomplete manifest");
        assert!(parsed(&text(&[("vs", "core_static", "../a.vs", '1', '2')])).is_err());
    }

    unsafe extern "C" fn read(user: *const c_void, index: u32, out: *mut Root) -> bool {
        let roots = unsafe { &*user.cast::<Vec<(String, String, String, &str, u32)>>() };
        let Some((name, original, motion, tag, stage)) = roots.get(index as usize) else {
            return false;
        };
        unsafe {
            out.write(Root {
                name: Bytes::from(name.as_bytes()),
                original: Bytes::from(original.as_bytes()),
                motion: Bytes::from(motion.as_bytes()),
                tag: Bytes::from(tag.as_bytes()),
                stage: *stage,
            })
        };
        true
    }

    unsafe extern "C" fn remember(user: *mut c_void, message: Bytes) {
        let out = unsafe { &mut *user.cast::<String>() };
        *out = String::from_utf8_lossy(unsafe { message.as_slice() }).into_owned();
    }

    #[test]
    fn pack_admission_owns_runtime_metadata_and_reports_refusals() {
        let mut roots: Vec<(String, String, String, &str, u32)> = vec![
            (
                "dx11/tess.ds".into(),
                hash('2'),
                hash('1'),
                "tess_domain",
                100,
            ),
            (
                "dx11/tess.hs".into(),
                hash('1'),
                hash('2'),
                "tess_hull",
                104,
            ),
        ];
        let mut error = String::new();
        let refusal = Refusal {
            user: (&raw mut error).cast(),
            report: remember,
        };
        let admit = |roots: &Vec<_>, count| unsafe {
            rsf_anomaly_shader_pack_manifest(
                Bytes::from(NS.as_bytes()),
                count,
                std::ptr::from_ref(roots).cast(),
                read,
                &refusal,
            )
        };
        let admitted = admit(&roots, 2);
        assert!(!admitted.is_null());
        // The Runtime strings may go away once admission returns.
        roots[0].0 = "changed".into();
        roots[0].1 = hash('7');
        let manifest = unsafe { &*admitted };
        let mut view = View {
            namespace: Bytes::from(&[][..]),
            entries: std::ptr::null(),
            count: 0,
        };
        rsf_anomaly_shader_admitted_view(manifest, &mut view);
        assert_eq!(view.count, 2);
        let domain = unsafe {
            rsf_anomaly_shader_admitted_find(manifest, Bytes::from(hash('2').as_bytes()))
        };
        assert_eq!(domain, unsafe { view.entries.add(1) });
        let domain = unsafe { &*domain };
        assert_eq!(
            (domain.name(), domain.stage, domain.family),
            (&b"dx11/tess.ds"[..], b'd', 7)
        );
        let found = unsafe {
            rsf_anomaly_shader_admitted_find(manifest, Bytes::from(hash('7').as_bytes()))
        };
        assert!(found.is_null());
        unsafe { rsf_anomaly_shader_admitted_free(admitted) };

        roots[0].1 = hash('1');
        assert!(admit(&roots, 2).is_null());
        assert_eq!(error, "ambiguous root source");
        roots[0].1 = hash('2');
        roots[0].4 = u32::from(b'h');
        assert!(admit(&roots, 2).is_null());
        assert_eq!(error, "invalid adapter root metadata");
        assert!(admit(&roots, 3).is_null());
        assert_eq!(error, "invalid adapter root metadata");
        roots[0].4 = u32::from(b'd');
        assert!(admit(&roots, 3).is_null());
        assert_eq!(error, "pack root metadata");
        assert!(admit(&roots, 0).is_null());
        assert_eq!(error, "incomplete pack manifest");
    }
}
