// SPDX-License-Identifier: MIT
fn main() {
    println!("cargo:rerun-if-changed=src/plugin_callbacks.cpp");
    println!("cargo:rerun-if-changed=src/fg_leases_com.cpp");
    println!("cargo:rerun-if-changed=src/settings_store_format.cpp");
    println!("cargo:rerun-if-changed=../../sdk/game/include");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut build = cc::Build::new();
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            // The final CMake or Cargo binary selects its CRT, including Debug.
            build.flags(["/EHsc", "/Zl"]);
        }
        build
            .cpp(true)
            .warnings_into_errors(true)
            .include("../../sdk/game/include")
            .file("src/plugin_callbacks.cpp")
            .file("src/fg_leases_com.cpp")
            .file("src/settings_store_format.cpp")
            .compile("rsf_orchestrator_native");
    }
}
