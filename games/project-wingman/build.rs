// SPDX-License-Identifier: MIT
//! Compiles the native exception leaves for the executables Cargo links (unit tests). The archive
//! is linked without bundling, so the staticlib CMake links never carries it: the adapter DLL
//! compiles the same file itself.
fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=src/native_platform.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut build = cc::Build::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.flags(["/Zl", "/std:c11"]);
    }
    build
        .includes(["../../sdk/game/include", "../../runtime/common/include"])
        .file("src/native_platform.c")
        .warnings_into_errors(true)
        .cargo_metadata(false)
        .compile("rsf_wingman_native");
    let out = std::env::var("OUT_DIR").expect("Cargo output directory");
    println!("cargo::rustc-link-search=native={out}");
    println!("cargo::rustc-link-lib=static:-bundle=rsf_wingman_native");
}
