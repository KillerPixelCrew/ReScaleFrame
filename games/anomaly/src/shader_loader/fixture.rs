// SPDX-License-Identifier: MIT
use super::manifest::{Root, admit_pack, parse};
use super::reflection::fixture::{
    compatibility, core, foliage_layout, tess_compatibility, tess_linkage,
};
use super::runtime_pack::admit_runtime_variant;
use super::tree::{cache_key, prepare_tree, rsf_anomaly_shader_match_request};
use super::*;
use crate::shader_family::{CORE_STATIC, TESS_DOMAIN, TESS_HULL};
use rsf_game_sys::{RSF_PATCH_OK, RSF_PATCH_ORIGINAL};
use rsf_windows::sha256_hex;
use std::cell::Cell;
use std::ffi::c_void;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

fn digest(bytes: &[u8]) -> String {
    String::from_utf8(sha256_hex(bytes).expect("fixture digest").to_vec()).expect("ASCII digest")
}

fn manifest_header(digit: char) -> String {
    format!(
        "rsf-anomaly-motion-manifest 1\nnamespace {}\n",
        digit.to_string().repeat(64)
    )
}

fn manifest_contract() {
    let h1 = b"1".repeat(64);
    let h2 = b"2".repeat(64);
    let ns = b"a".repeat(64);
    let a = String::from_utf8_lossy(&h1);
    let b = String::from_utf8_lossy(&h2);
    let head = manifest_header('a');
    let text = format!(
        "{}root vs core_static deffer_base_flat.vs {a} {b}\n",
        head.replacen('\n', "\r\n", 1)
    );
    let manifest = Arc::new(parse(text.as_bytes()).expect("valid manifest"));
    assert_eq!(manifest.namespace(), ns.as_slice());
    assert_eq!(manifest.entries().len(), 1);
    let row = manifest.find(&h1).expect("root hash");
    assert!(std::ptr::eq(row, &manifest.entries()[0]));
    assert_eq!(row.family, CORE_STATIC);
    assert_eq!(row.motion(), h2.as_slice());
    assert!(manifest.find(&h2).is_none() && manifest.find(&h1[1..]).is_none());
    let row = std::ptr::from_ref(row);
    let shared = Arc::clone(&manifest);
    drop(manifest);
    let retained = shared.find(&h1).unwrap();
    assert!(std::ptr::eq(retained, row));
    assert_eq!(retained.name(), b"deffer_base_flat.vs");
    for invalid in [
        String::from("rsf-anomaly-motion-manifest 2\n"),
        format!("{head}root ps core_static x.ps {a} {b}\n"),
        format!("{head}root vs core_static ../x.vs {a} {b}\n"),
        format!("{head}root vs core_static a.vs {a} {b}\nroot vs core_model b.vs {a} {b}\n"),
        head.clone(),
        format!("{head}root vs core_static a.vs {a} XYZ\n"),
    ] {
        assert!(parse(invalid.as_bytes()).is_err(), "admitted {invalid}");
    }

    unsafe extern "C" fn read(user: *const c_void, index: u32, out: *mut Root) -> bool {
        let rows = unsafe { &*user.cast::<[Root; 2]>() };
        let Some(row) = rows.get(index as usize) else {
            return false;
        };
        unsafe {
            out.write(Root {
                name: row.name,
                original: row.original,
                motion: row.motion,
                tag: row.tag,
                stage: row.stage,
            });
        }
        true
    }
    let mut roots = [
        Root {
            name: Bytes::from(b"dx11/tess.hs".as_slice()),
            original: Bytes::from(h1.as_slice()),
            motion: Bytes::from(h2.as_slice()),
            tag: Bytes::from(b"tess_hull".as_slice()),
            stage: u32::from(b'h'),
        },
        Root {
            name: Bytes::from(b"dx11/tess.ds".as_slice()),
            original: Bytes::from(h2.as_slice()),
            motion: Bytes::from(h1.as_slice()),
            tag: Bytes::from(b"tess_domain".as_slice()),
            stage: u32::from(b'd'),
        },
    ];
    let admit =
        |roots: &[Root; 2]| unsafe { admit_pack(&ns, 2, std::ptr::from_ref(roots).cast(), read) };
    let pack = admit(&roots).expect("valid Runtime roots");
    assert_eq!(pack.entries().len(), 2);
    assert_eq!(pack.find(&h1).unwrap().family, TESS_HULL);
    assert_eq!(pack.find(&h2).unwrap().family, TESS_DOMAIN);
    assert_eq!(pack.entries()[0].name(), b"dx11/tess.hs");
    assert_eq!(pack.entries()[1].name(), b"dx11/tess.ds");
    roots[1].stage = u32::from(b'h');
    assert!(admit(&roots).is_err());
    roots[1].stage = u32::from(b'd');
    roots[1].original = Bytes::from(h1.as_slice());
    assert_eq!(
        admit(&roots).err().as_deref(),
        Some("ambiguous root source")
    );
    assert_eq!(pack.entries().len(), 2);
    assert_eq!(pack.find(&h2).unwrap().stage, b'd');
}

fn tess_manifest() {
    let head = manifest_header('a');
    let h1 = "1".repeat(64);
    let h2 = "2".repeat(64);
    let h3 = "3".repeat(64);
    let h4 = "4".repeat(64);
    let text = format!(
        "{head}root hs tess_hull dx11/tess.hs {h1} {h2}\nroot ds tess_domain dx11/tess.ds {h3} {h4}\n"
    );
    let valid = parse(text.as_bytes()).expect("valid tessellation roots");
    let hull = valid.find(h1.as_bytes()).unwrap();
    let domain = valid.find(h3.as_bytes()).unwrap();
    assert_eq!((hull.family, hull.stage), (TESS_HULL, b'h'));
    assert_eq!(
        (domain.stage, domain.name()),
        (b'd', b"dx11/tess.ds".as_slice())
    );
    for bad in [
        "root vs tess_hull dx11/tess.hs ",
        "root ds tess_hull dx11/tess.hs ",
        "root gs tess_hull dx11/tess.hs ",
        "root hs tess_hull /tess.hs ",
        "root hs tess_hull dx11/ ",
        "root hs tess_hull a/b/tess.hs ",
        "root hs tess_hull dx11\\tess.hs ",
        "root hs tess_hull dx11/../tess.hs ",
    ] {
        assert!(
            parse(format!("{head}{bad}{h1} {h2}\n").as_bytes()).is_err(),
            "admitted {bad}"
        );
    }
}

#[derive(Default)]
struct Compiler {
    calls: Cell<u32>,
    fail_motion: bool,
    drift: bool,
}

impl Compiler {
    fn compile(&self, motion: bool, source: &[u8], request: &Request) -> Result<Vec<u8>, Vec<u8>> {
        self.calls.set(self.calls.get() + 1);
        if motion && self.fail_motion {
            return Err(b"fake failure".to_vec());
        }
        let leaf = if motion { "motion" } else { "original" };
        let mut bytes = format!("DXBC|{leaf}|").into_bytes();
        bytes.extend(source);
        for span in [request.entry, request.target] {
            bytes.push(b'|');
            bytes.extend(unsafe { span.as_slice() });
        }
        bytes.extend(format!("|{}|", request.flags).bytes());
        for define in unsafe { request.defines.as_slice() } {
            bytes.extend(unsafe { define.name.as_slice() });
            bytes.push(b'=');
            bytes.extend(unsafe { define.value.as_slice() });
            bytes.push(b';');
        }
        if !motion && self.drift {
            bytes.push(b'x');
        }
        Ok(bytes)
    }
}

fn reflect(code: &[u8], incompatible: bool) -> Option<Snapshot> {
    let text = std::str::from_utf8(code).ok()?;
    if !text.starts_with("DXBC|") && !text.starts_with("STOCK|") {
        return None;
    }
    let variant = text.contains("|motion|");
    let stage = if text.contains("|vs_5_0|") {
        b'v'
    } else {
        b'p'
    };
    let mut reflected = core(stage, variant);
    if variant && incompatible {
        reflected.break_buffer();
    }
    Some(reflected.snapshot())
}

fn request(target: &[u8], source: &[u8], stock: &[u8], defines: &[Define]) -> Request {
    Request {
        target: Bytes::from(target),
        source: Bytes::from(source),
        bytecode: Bytes::from(stock),
        defines: Span::from(defines),
        entry: Bytes::from(b"main".as_slice()),
        flags: 8,
    }
}

struct Deployment(PathBuf);

impl Drop for Deployment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn loader_contract() {
    let root = std::env::temp_dir().join(format!("rsf-motion-contract-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    let deployment = Deployment(root);
    let root = &deployment.0;
    let cache = root.join("cache");
    let source = b"VS root source";
    let motion = b"VS motion root";
    let pixel = b"PS root source";
    let tree = root.join("motion").join("r3");
    fs::create_dir_all(&tree).unwrap();
    fs::write(tree.join("deffer_base_flat.vs"), motion).unwrap();
    let head = manifest_header('c');
    let meta = format!(
        "{head}root vs core_static deffer_base_flat.vs {} {}\nroot ps core_pixel deffer_base_flat.ps {} {}\n",
        digest(source),
        digest(motion),
        digest(pixel),
        "e".repeat(64),
    );
    let manifest = parse(meta.as_bytes()).unwrap();
    let entry = |request: &Request| unsafe {
        rsf_anomaly_shader_match_request(Some(&manifest), request.source, request.target)
    };
    let defines = [Define {
        name: Bytes::from(b"SMAP_size".as_slice()),
        value: Bytes::from(b"2048".as_slice()),
    }];
    let mut compiler = Compiler::default();
    let unlisted = request(b"vs_5_0", b"unrelated root", &[], &defines);
    assert!(entry(&unlisted).is_none());
    assert_eq!(compiler.calls.get(), 0);
    let vs = request(b"vs_5_0", source, &[], &defines);
    let stock = compiler.compile(false, source, &vs).unwrap();
    compiler.calls.set(0);
    let vs = request(b"vs_5_0", source, &stock, &defines);
    let admitted = entry(&vs).unwrap();
    assert_eq!(admitted.family, CORE_STATIC);
    let wrong_stage = request(b"ps_5_0", source, &stock, &defines);
    assert!(entry(&wrong_stage).is_none());
    assert!(
        entry(&request(
            b"vs_5_0",
            b"unlisted Runtime root",
            &stock,
            &defines
        ))
        .is_none()
    );

    let runtime_bytes = b"DXBC|motion|root|main|vs_5_0|8|";
    let runtime = |status, incompatible| {
        admit_runtime_variant(
            &vs,
            admitted.stage,
            status,
            true,
            || runtime_bytes,
            |code| reflect(code, incompatible),
        )
    };
    let accepted = runtime(RSF_PATCH_OK, false)
        .ok()
        .expect("compatible Runtime cache hit");
    assert_eq!(accepted.outcome, Outcome::ReplacedFromCache as u8);
    assert_eq!(accepted.bytecode, runtime_bytes.as_slice());
    assert!(matches!(
        runtime(RSF_PATCH_ORIGINAL, false),
        Err(Outcome::OriginalMismatch)
    ));
    assert!(matches!(
        runtime(RSF_PATCH_OK, true),
        Err(Outcome::ReflectionMismatch)
    ));
    assert_eq!(compiler.calls.get(), 0);

    let id = admitted.identity(manifest.namespace());
    let prepare = |request, id, root: &Path, cache: &Path, compiler: &Compiler, incompatible| {
        prepare_tree(
            request,
            id,
            root,
            Some(cache),
            |_, code| reflect(code, incompatible),
            Snapshot::view,
            |patched, code| compiler.compile(patched, code, request),
        )
    };
    let prepared = prepare(&vs, &id, root, &cache, &compiler, false);
    assert!(matches!(prepared.outcome, Outcome::Replaced));
    assert_eq!(compiler.calls.get(), 2);
    assert_eq!(
        prepared.bytes,
        b"DXBC|motion|VS motion root|main|vs_5_0|8|SMAP_size=2048;".as_slice()
    );
    let key = cache_key(&vs, &id).unwrap();
    assert_eq!(
        &key,
        b"9d93a1707dc4402f46d0e05821221752f0ed091580cbafe9d0945137ba6519e7"
    );
    let cached = cache.join(format!("{}.dxbc", std::str::from_utf8(&key).unwrap()));
    assert!(cached.is_file());
    compiler.calls.set(0);
    assert!(matches!(
        prepare(&vs, &id, root, &cache, &compiler, false).outcome,
        Outcome::ReplacedFromCache
    ));
    assert_eq!(compiler.calls.get(), 0);
    let more_defines = [
        defines[0],
        Define {
            name: Bytes::from(b"SKIN_1".as_slice()),
            value: Bytes::from(b"1".as_slice()),
        },
    ];
    assert_ne!(
        cache_key(&request(b"vs_5_0", source, &stock, &more_defines), &id),
        Some(key)
    );
    fs::write(&cached, "garbage").unwrap();
    let prepared = prepare(&vs, &id, root, &cache, &compiler, false);
    assert!(matches!(prepared.outcome, Outcome::Replaced));
    assert_eq!(compiler.calls.get(), 2);
    fs::remove_dir_all(&cache).unwrap();
    compiler.drift = true;
    let refused = prepare(&vs, &id, root, &cache, &compiler, false);
    assert!(matches!(refused.outcome, Outcome::OriginalMismatch) && refused.bytes.is_empty());
    compiler.drift = false;
    compiler.fail_motion = true;
    assert!(matches!(
        prepare(&vs, &id, root, &cache, &compiler, false).outcome,
        Outcome::MotionCompileFailed
    ));
    compiler.fail_motion = false;
    let refused = prepare(&vs, &id, root, &cache, &compiler, true);
    assert!(
        matches!(refused.outcome, Outcome::ReflectionMismatch)
            && refused.compatibility == Compatibility::Buffer
    );
    let ps = request(b"ps_5_0", pixel, &[], &defines);
    let stock = compiler.compile(false, pixel, &ps).unwrap();
    let ps = request(b"ps_5_0", pixel, &stock, &defines);
    let pixel_id = entry(&ps).unwrap().identity(manifest.namespace());
    assert!(matches!(
        prepare(&ps, &pixel_id, root, &cache, &compiler, false).outcome,
        Outcome::MotionCompileFailed
    ));

    let sub_source = b"sub root source";
    let sub_motion = b"sub motion root";
    fs::create_dir_all(tree.join("dx11")).unwrap();
    fs::write(tree.join("dx11").join("deffer_base_flat_d.vs"), sub_motion).unwrap();
    let sub_meta = format!(
        "{head}root vs core_static dx11/deffer_base_flat_d.vs {} {}\n",
        digest(sub_source),
        digest(sub_motion)
    );
    let sub_manifest = parse(sub_meta.as_bytes()).unwrap();
    let sub_id = sub_manifest
        .find(&sha256_hex(sub_source).unwrap())
        .unwrap()
        .identity(sub_manifest.namespace());
    let sub = request(b"vs_5_0", sub_source, &[], &defines);
    let stock = compiler.compile(false, sub_source, &sub).unwrap();
    let sub = request(b"vs_5_0", sub_source, &stock, &defines);
    // canonicalize returns the extended-length Windows spelling (\\?\).
    let extended = root.canonicalize().unwrap();
    assert!(matches!(
        prepare(
            &sub,
            &sub_id,
            &extended,
            &extended.join("cache"),
            &compiler,
            false
        )
        .outcome,
        Outcome::Replaced
    ));
}

pub(crate) const SHADER_CONTRACT_CASES: &[(&str, fn())] = &[
    ("compatibility", compatibility),
    ("foliage-layout-reflection", foliage_layout),
    ("loader", loader_contract),
    ("manifest", manifest_contract),
    ("tess-compatibility", tess_compatibility),
    ("tess-linkage", tess_linkage),
    ("tess-manifest", tess_manifest),
];
