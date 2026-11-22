// SPDX-License-Identifier: MIT
//! Compiles the native leaves (detours and exception boundaries) for the
//! executables Cargo links (unit tests). The archive is linked without bundling, so the staticlib
//! CMake links never carries it: CMake compiles the same files as rsf_skyrim_se_native.
const LEAVES: [&str; 4] = [
    "src/render_detours.c",
    "src/native_creation.c",
    "src/compatibility.c",
    "src/cpu_events.c",
];

fn main() {
    bindings();
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=src");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut build = cc::Build::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.flags(["/Zl", "/std:c11"]);
    }
    build
        .includes([
            "src",
            "../../sdk/game/include",
            "../../runtime/common/include",
            "../../runtime/orchestrator/include",
            "../../runtime/graphics/include",
            "../../runtime/presentation/include",
            "../../integrations/steam/include",
        ])
        .files(LEAVES)
        .warnings_into_errors(true)
        .cargo_metadata(false)
        .compile("rsf_skyrim_se_native");
    let out = std::env::var("OUT_DIR").expect("Cargo output directory");
    println!("cargo::rustc-link-search=native={out}");
    println!("cargo::rustc-link-lib=static:-bundle=rsf_skyrim_se_native");
}

/// Binds the Skyrim CPU call header (this binding had no generator before).
fn bindings() {
    fn derives(name: &str) -> &'static [&'static str] {
        match name {
            "rsf_skyrim_cpu_call" | "rsf_skyrim_menu_observation" => &["Debug"],
            _ => &[],
        }
    }
    let builder = rsf_bindgen::owner(
        &["games/skyrim-se/src/cpu_call.h"],
        &[],
        &["games/skyrim-se"],
        &[],
        &[],
    )
    .parse_callbacks(Box::new(rsf_bindgen::Derives(derives)))
    .prepend_enum_name(false)
    // The adapter declares the C calls it makes by hand.
    .ignore_functions();
    rsf_bindgen::write(builder, "sys.rs");
}
