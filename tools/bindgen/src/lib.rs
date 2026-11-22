// SPDX-License-Identifier: MIT
//! Runs bindgen from a binding owner's build script. Every owner generates the items of its own
//! headers once; other crates depend on the owner instead of binding the same header again.
//! See docs/tooling.md for the pinned libclang.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::{env, fs};

pub use bindgen;
use bindgen::callbacks::{DeriveInfo, DeriveTrait, ImplementsTrait, ParseCallbacks};

/// The LLVM release docs/tooling.md and .github/workflows/verify.yml install.
const LLVM_HINT: &str = "bindgen needs libclang. Install LLVM 22.1.8 \
    (`winget install --id LLVM.LLVM --exact --version 22.1.8`) or set LIBCLANG_PATH to the \
    directory that holds libclang.dll; see docs/tooling.md";

/// Public include directories whose `rescaleframe/*.h` headers rsf-game-sys binds. Game and
/// integration headers stay with their crates (Rule 1). Other owners blocklist these files and
/// take the items from rsf-game-sys.
pub const PUBLIC_INCLUDE_DIRS: &[&str] = &[
    "sdk/game/include",
    "runtime/orchestrator/include",
    "runtime/graphics/include",
    "runtime/presentation/include",
    "runtime/contract/include",
    "runtime/backends/dlss/include",
    "runtime/backends/fsr/include",
    "runtime/backends/xess/include",
    "runtime/common/include",
    "runtime/shader_patch/include",
    "ui/overlay/include",
    "loader/census/include",
    "loader/diagnostics/include",
];

/// Every public header under [`PUBLIC_INCLUDE_DIRS`], repository-relative.
pub fn public_headers() -> Vec<String> {
    let root = root();
    let mut headers = Vec::new();
    for dir in PUBLIC_INCLUDE_DIRS {
        let public = root.join(dir).join("rescaleframe");
        println!("cargo:rerun-if-changed={}", public.display());
        for entry in fs::read_dir(&public).expect("public include directory") {
            let name = entry.expect("directory entry").file_name();
            let name = name.to_str().expect("UTF-8 header name");
            if name.ends_with(".h") {
                headers.push(format!("{dir}/rescaleframe/{name}"));
            }
        }
    }
    headers.sort();
    headers
}

/// The repository root, found from the calling crate's manifest directory.
pub fn root() -> PathBuf {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .find(|path| path.join("Cargo.lock").is_file())
        .expect("repository root with Cargo.lock")
        .to_path_buf()
}

fn find_libclang() {
    println!("cargo:rerun-if-env-changed=LIBCLANG_PATH");
    let has = |dir: &Path| dir.join("libclang.dll").is_file() || dir.join("libclang.so").is_file();
    if let Some(dir) = env::var_os("LIBCLANG_PATH") {
        assert!(
            has(Path::new(&dir)),
            "LIBCLANG_PATH={dir:?} has no libclang. {LLVM_HINT}"
        );
        return;
    }
    let path = env::var_os("PATH").unwrap_or_default();
    let found = env::split_paths(&path)
        .chain([PathBuf::from(r"C:\Program Files\LLVM\bin")])
        .find(|dir| has(dir));
    match found {
        // SAFETY: the build script is single-threaded at this point.
        Some(dir) => unsafe { env::set_var("LIBCLANG_PATH", dir) },
        None if cfg!(windows) => panic!("{LLVM_HINT}"),
        // Elsewhere clang-sys also searches the usual system locations.
        None => {}
    }
}

/// A builder over `headers` (repository-relative) with `include_dirs` on the include path and
/// the settings every RSF binding shares. The caller adds its allowlist and owner options.
pub fn builder(headers: &[&str], include_dirs: &[&str]) -> bindgen::Builder {
    find_libclang();
    let root = root();
    let umbrella: String = headers
        .iter()
        .map(|header| format!("#include \"{}\"\n", root.join(header).display()))
        .collect();
    // The committed bindings were always generated for the MSVC layout; the GNU target used by
    // the VulkanStory DLLs has the same x64 layout for these records.
    let target = if cfg!(windows) {
        "x86_64-pc-windows-msvc".to_owned()
    } else {
        env::var("TARGET").expect("TARGET")
    };
    let mut builder = bindgen::Builder::default()
        .header_contents("rsf_bindings.h", &umbrella)
        .clang_arg(format!("--target={target}"))
        .use_core()
        .derive_debug(false)
        .derive_default(true)
        .generate_comments(false)
        .rust_target(bindgen::RustTarget::stable(85, 0).expect("Rust 1.85 target"))
        .rust_edition(bindgen::RustEdition::Edition2024)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));
    for dir in include_dirs {
        builder = builder.clang_arg(format!("-I{}", root.join(dir).display()));
    }
    builder
}

/// A builder for a crate that owns the headers under `own` (repository-relative directories, or
/// single `.h` files). It binds only the items those files declare. Items from `provided`
/// (directories another owner binds) and from the public headers are left out and come from that
/// owner's `sys` module, which the crate's `sys` re-exports. `clang_args` reach both parses.
///
/// The public headers are parsed too, so that a forward declaration of a public record in an owned
/// header names the public definition instead of becoming an opaque local struct. Clang names a
/// header the way it was included, `..` segments and all, so a first parse collects every included
/// file and the lists name each file exactly as clang spells it.
pub fn owner(
    headers: &[&str],
    include_dirs: &[&str],
    own: &[&str],
    provided: &[&str],
    clang_args: &[&str],
) -> bindgen::Builder {
    let includes: Vec<&str> = PUBLIC_INCLUDE_DIRS
        .iter()
        .chain(include_dirs)
        .copied()
        .collect();
    let blocked: Vec<&str> = PUBLIC_INCLUDE_DIRS
        .iter()
        .chain(provided)
        .copied()
        .filter(|entry| !own.contains(entry))
        .collect();
    let public = public_headers();
    let all: Vec<&str> = public
        .iter()
        .map(String::as_str)
        .chain(headers.iter().copied())
        .collect();
    let base = builder(&all, &includes).clang_args(clang_args);
    let seen = Arc::new(Mutex::new(Vec::new()));
    base.clone()
        .allowlist_item("rsf_bindgen_collects_includes_only")
        .parse_callbacks(Box::new(Includes(Arc::clone(&seen))))
        .generate()
        .unwrap_or_else(|error| panic!("bindgen could not parse {headers:?}: {error}"));
    let root = fs::canonicalize(root()).expect("canonical repository root");
    let under = |entries: &[&str], path: &str| {
        entries
            .iter()
            .any(|entry| path == *entry || path.starts_with(&format!("{entry}/")))
    };
    let (mut allow, mut block) = (Vec::new(), Vec::new());
    for spelled in seen.lock().unwrap().iter() {
        let Ok(path) = fs::canonicalize(spelled) else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(&root) else {
            continue; // A system or SDK header.
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let exact = own.contains(&relative.as_str());
        if exact || (under(own, &relative) && !under(&blocked, &relative)) {
            allow.push(regex::escape(spelled));
        } else if under(&blocked, &relative) {
            block.push(regex::escape(spelled));
        }
    }
    assert!(!allow.is_empty(), "none of {headers:?} is under {own:?}");
    let mut builder = base.allowlist_file(allow.join("|"));
    if !block.is_empty() {
        builder = builder
            .blocklist_file(block.join("|"))
            .parse_callbacks(Box::new(ProvidedRecords));
    }
    builder
}

/// The records another owner binds are plain C data: Copy (except the one rsf-game-sys keeps
/// move-only) and Default. Without this, bindgen drops those derives from every record that
/// embeds one.
#[derive(Debug)]
struct ProvidedRecords;

impl ParseCallbacks for ProvidedRecords {
    fn blocklisted_type_implements_trait(
        &self,
        name: &str,
        derive_trait: DeriveTrait,
    ) -> Option<ImplementsTrait> {
        match derive_trait {
            DeriveTrait::Copy if name == "rsf_d3d11_state" => Some(ImplementsTrait::No),
            DeriveTrait::Copy | DeriveTrait::Default => Some(ImplementsTrait::Yes),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct Includes(Arc<Mutex<Vec<String>>>);

impl ParseCallbacks for Includes {
    fn include_file(&self, filename: &str) {
        self.0.lock().unwrap().push(filename.to_owned());
    }
}

/// Extra derives for named records, the build-script form of `--with-derive-custom-struct`.
#[derive(Debug)]
pub struct Derives(pub fn(&str) -> &'static [&'static str]);

impl ParseCallbacks for Derives {
    fn add_derives(&self, info: &DeriveInfo<'_>) -> Vec<String> {
        (self.0)(info.name)
            .iter()
            .map(|d| (*d).to_owned())
            .collect()
    }
}

/// Generates `builder` into `$OUT_DIR/<file>`.
pub fn write(builder: bindgen::Builder, file: &str) {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join(file);
    builder
        .generate()
        .unwrap_or_else(|error| panic!("bindgen failed for {file}: {error}"))
        .write_to_file(&out)
        .unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
}
