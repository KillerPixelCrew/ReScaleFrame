// SPDX-License-Identifier: MIT
fn main() {
    let sources = [
        "src/plugin_callbacks.cpp",
        "src/com_ref.cpp",
        "src/fg_leases_com.cpp",
        "src/sampler_bias_com.cpp",
        "src/settings_store_format.cpp",
        "src/d3d11_output_platform.cpp",
        "src/native_regions_com.cpp",
        "src/native_translucency_com.cpp",
    ];
    for source in sources {
        println!("cargo:rerun-if-changed={source}");
    }
    println!("cargo:rerun-if-changed=src/d3d11_output_platform.h");
    println!("cargo:rerun-if-changed=../graphics/include/rescaleframe/d3d11_state.h");
    println!("cargo:rerun-if-changed=../graphics/include/rescaleframe/srv_format.h");
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
            .include("../graphics/include")
            .files(sources)
            .compile("rsf_orchestrator_native");
    }
}
