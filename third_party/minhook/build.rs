// SPDX-License-Identifier: MIT
//! Compiles the pinned MinHook for the binaries Cargo links (unit tests and tools). The archive is
//! linked without bundling, so the staticlibs CMake links never carry it: every CMake binary links
//! the one `minhook` target that CMake builds from the same revision.
//!
//! The source is the `vendor/minhook` submodule, pinned to the revision of `minhook.lock.json`
//! and `eng/dependencies.json`; git's commit pin vouches for its files, as for the other vendor
//! submodules, so the build needs neither CMake nor the network. A checkout without that
//! submodule falls back to `minhook.cmake`, which downloads the locked files into `OUT_DIR` and
//! checks their SHA-256 (CMake on PATH and network at the first build).
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=minhook.cmake");
    println!("cargo::rerun-if-changed=minhook.lock.json");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let vendor = manifest.join("../../vendor/minhook");
    println!(
        "cargo::rerun-if-changed={}",
        vendor.join("src/hook.c").display()
    );
    let source = if vendor.join("src/hook.c").exists() {
        vendor
    } else {
        let fetched = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"))
            .join("minhook");
        let status = Command::new("cmake")
            .arg(format!("-DRSF_MINHOOK={}", fetched.display()))
            .arg("-P")
            .arg(manifest.join("minhook.cmake"))
            .status()
            .expect(
                "vendor/minhook is not checked out (git submodule update --init --depth 1 \
                 vendor/minhook), and CMake to fetch the pinned MinHook could not run",
            );
        assert!(status.success(), "MinHook provisioning failed");
        fetched
    };
    let hde = if std::env::var("CARGO_CFG_TARGET_POINTER_WIDTH").as_deref() == Ok("64") {
        "src/hde/hde64.c"
    } else {
        "src/hde/hde32.c"
    };
    let mut build = cc::Build::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.flag("/Zl");
    }
    build
        .include(source.join("include"))
        .include(source.join("src"))
        .files(
            ["src/buffer.c", "src/hook.c", "src/trampoline.c", hde].map(|file| source.join(file)),
        )
        .warnings(false)
        .cargo_metadata(false)
        .compile("rsf_minhook");
    let out = std::env::var("OUT_DIR").expect("Cargo output directory");
    // DEP_MINHOOK_INCLUDE, for dependents whose C leaves include MinHook.h.
    println!(
        "cargo::metadata=include={}",
        source.join("include").display()
    );
    println!("cargo::rustc-link-search=native={out}");
    println!("cargo::rustc-link-lib=static:-bundle=rsf_minhook");
}
