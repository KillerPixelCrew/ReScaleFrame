// SPDX-License-Identifier: MIT
/// Binds the two private headers this crate's leaves implement or call. The public headers come
/// from rsf-game-sys; the Runtime takes these from here.
fn bindings() {
    let builder = rsf_bindgen::owner(
        &[
            "runtime/orchestrator/src/d3d11_output_platform.h",
            "runtime/graphics/src/frame_tap_platform.h",
        ],
        &[],
        &[
            "runtime/orchestrator/src/d3d11_output_platform.h",
            "runtime/graphics/src/frame_tap_platform.h",
        ],
        &[],
        &[],
    );
    rsf_bindgen::write(builder, "sys.rs");
}

// C leaf: contains foreign C++ exceptions raised by borrowed frame-tap callbacks. It is bundled into
// this crate, so the adapter archive and the Runtime archive that embeds the rlib both carry it.
fn main() {
    bindings();
    println!("cargo:rerun-if-changed=../src/frame_tap_callbacks.c");
    println!("cargo:rerun-if-changed=../src/frame_tap_platform.h");
    println!("cargo:rerun-if-changed=../../orchestrator/src/d3d11_output_platform.h");
    println!("cargo:rerun-if-changed=../include/rescaleframe/frame_tap.h");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut build = cc::Build::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.flags(["/Zl", "/std:c11"]);
    }
    build
        .include("../../../sdk/game/include")
        .include("../include")
        .define("WIN32_LEAN_AND_MEAN", None)
        .warnings_into_errors(true)
        .file("../src/frame_tap_callbacks.c")
        .compile("rsf_frame_tap_callbacks");
}
