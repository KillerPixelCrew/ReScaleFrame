// SPDX-License-Identifier: MIT
//! Compiles the exception guard leaf for the executables Cargo links (unit tests). The archive is
//! linked without bundling, so the staticlibs CMake links never carry it: CMake compiles
//! exception_guards.c through rsf_steam_native.
fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=src/exception_guards.c");
    println!("cargo::rerun-if-changed=include");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut build = cc::Build::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.flags(["/Zl", "/std:c11"]);
    }
    build
        .includes([
            "include",
            "../../sdk/game/include",
            "../../runtime/orchestrator/include",
            "../../runtime/graphics/include",
            "../../runtime/presentation/include",
        ])
        .file("src/exception_guards.c")
        .warnings_into_errors(true)
        .cargo_metadata(false)
        .compile("rsf_steam_native");
    let out = std::env::var("OUT_DIR").expect("Cargo output directory");
    println!("cargo::rustc-link-search=native={out}");
    println!("cargo::rustc-link-lib=static:-bundle=rsf_steam_native");
}
