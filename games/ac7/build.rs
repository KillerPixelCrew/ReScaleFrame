// SPDX-License-Identifier: MIT
//! Compiles the native leaves for the executables Cargo links (unit tests and the capture fixture's
//! test link): the guarded memory leaf and the motion-capture trampolines. Each archive is linked
//! without bundling, so the staticlib CMake links into the adapter does not carry them: CMake builds
//! the same files as rsf_ac7_memory and rsf_ac7_capture_trampolines.
fn leaf(name: &str, files: &[&str], includes: &[&str]) {
    for file in files {
        println!("cargo::rerun-if-changed={file}");
    }
    let mut build = cc::Build::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.flags(["/Zl", "/std:c11"]);
    } else {
        build.std("c11");
    }
    build
        .includes(includes)
        .files(files)
        .warnings_into_errors(true)
        .cargo_metadata(false)
        .compile(name);
    println!("cargo::rustc-link-lib=static:-bundle={name}");
}

fn main() {
    bindings();
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=include");
    let out = std::env::var("OUT_DIR").expect("Cargo output directory");
    println!("cargo::rustc-link-search=native={out}");
    leaf("rsf_ac7_memory", &["src/ac7_memory.c"], &["include"]);
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let minhook = std::env::var("DEP_MINHOOK_INCLUDE").expect("rsf-minhook-sys include directory");
    leaf(
        "rsf_ac7_capture_trampolines",
        &[
            "src/motion_capture_trampolines.c",
            "src/ac7_hooks.c",
        ],
        &["include", "src", "../../sdk/game/include", &minhook],
    );
}

/// Binds the AC7 adapter headers, public and private. The test crates and standalone fixtures
/// use these items from this crate.
fn bindings() {
    let mut headers: Vec<String> = std::fs::read_dir("include/rescaleframe")
        .expect("AC7 public headers")
        .map(|entry| {
            entry
                .expect("directory entry")
                .file_name()
                .into_string()
                .unwrap()
        })
        .filter(|name| name.ends_with(".h"))
        .map(|name| format!("games/ac7/include/rescaleframe/{name}"))
        .collect();
    headers.sort();
    println!("cargo:rerun-if-changed=include/rescaleframe");
    for private in [
        "ac7_platform.h",
        "ac7_scope_owner.h",
        "ac7_sites.h",
        "cloud_depth_platform.h",
        "contact_shadow.h",
    ] {
        headers.push(format!("games/ac7/src/{private}"));
    }
    let headers: Vec<&str> = headers.iter().map(String::as_str).collect();
    let builder = rsf_bindgen::owner(&headers, &["games/ac7/include"], &["games/ac7"], &[], &[])
        // render_scope.rs defines these and native_renderer.rs declares them with Rust types.
        .blocklist_function("rsf_ac7_render_scope.*");
    rsf_bindgen::write(builder, "sys.rs");
}
