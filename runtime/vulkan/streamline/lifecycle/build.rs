// SPDX-License-Identifier: MIT
//! Compiles the C leaf of the Streamline call leaves: SEH containment and the
//! sl::FrameToken virtual call.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(rsf_streamline_core)");
    println!("cargo:rerun-if-changed=../sdk_guard.c");
    let mut build = cc::Build::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // The final CMake binary selects its CRT, including Debug.
        build.flags(["/Zl", "/std:c11"]);
    } else {
        build.std("c11");
    }
    build
        .warnings_into_errors(true)
        .file("../sdk_guard.c")
        .compile("rsf_slvk_guard");
}
