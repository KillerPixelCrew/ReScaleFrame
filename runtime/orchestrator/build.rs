// SPDX-License-Identifier: MIT
use std::path::PathBuf;
use std::process::Command;

fn shader_patch_parser() {
    let shader_patch = PathBuf::from("../shader_patch");
    let vendor = PathBuf::from("../../vendor/shader-patch/yyjson");
    let yyjson = if vendor.join("src/yyjson.c").exists() {
        vendor
    } else {
        PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory")).join("yyjson")
    };
    for file in [
        "src/json.c",
        "src/json.h",
        "include/rescaleframe/shader_patch.h",
        "yyjson.cmake",
        "yyjson.lock.json",
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            shader_patch.join(file).display()
        );
    }
    let status = Command::new("cmake")
        .arg(format!("-DRSF_PATCH_YYJSON={}", yyjson.display()))
        .arg("-P")
        .arg(shader_patch.join("yyjson.cmake"))
        .status()
        .expect("run CMake to provision the pinned shader-pack parser");
    assert!(status.success(), "shader-pack parser provisioning failed");
    let mut build = cc::Build::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.flag("/Zl");
        build.flag("/std:c11");
    }
    build
        .include(yyjson.join("src"))
        .include(shader_patch.join("include"))
        .file(yyjson.join("src/yyjson.c"))
        .file(shader_patch.join("src/json.c"))
        .warnings(false)
        .compile("rsf_patch_json");
}

fn main() {
    let sources = [
        "src/plugin_callbacks.cpp",
        "src/com_ref.cpp",
        "src/fg_leases_com.cpp",
        "src/sampler_bias_com.cpp",
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
        shader_patch_parser();
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
