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

fn sharpening_payload() {
    let directory = "../graphics/src/sharpening";
    for file in [
        "pass_payload.cpp",
        "d3d11.h",
        "options.h",
        "rcas.h",
        "da_rcas.h",
        "da_das.h",
        "rcas_spirv.h",
        "da_rcas_spirv.h",
        "da_das_spirv.h",
        "depth_color.inc",
    ] {
        println!("cargo:rerun-if-changed={directory}/{file}");
    }
    println!("cargo:rerun-if-changed=include/rescaleframe/sharpening_settings.h");
    let mut build = cc::Build::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.flags(["/EHsc", "/Zl"]);
    }
    build
        .cpp(true)
        .std("c++17")
        .include("include")
        .warnings_into_errors(true)
        .file(format!("{directory}/pass_payload.cpp"))
        .compile("rsf_sharpening_payload");
}

fn plugin_callbacks() {
    let mut build = cc::Build::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.flags(["/Zl", "/std:c11"]);
    }
    build
        .include("../../sdk/game/include")
        .warnings_into_errors(true)
        .file("src/plugin_callbacks.c")
        .compile("rsf_plugin_callbacks");
}

fn main() {
    let sources = ["src/frame_tap_callbacks.cpp"];
    for source in sources {
        println!("cargo:rerun-if-changed={source}");
    }
    println!("cargo:rerun-if-changed=src/plugin_callbacks.c");
    println!("cargo:rerun-if-changed=src/d3d11_output_platform.h");
    println!("cargo:rerun-if-changed=../graphics/include/rescaleframe/d3d11_state.h");
    println!("cargo:rerun-if-changed=../graphics/include/rescaleframe/srv_format.h");
    println!("cargo:rerun-if-changed=../../sdk/game/include");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        shader_patch_parser();
        sharpening_payload();
        plugin_callbacks();
        let mut build = cc::Build::new();
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            // The final CMake or Cargo binary selects its CRT, including Debug.
            build.flags(["/EHsc", "/Zl"]);
        }
        build
            .cpp(true)
            .std("c++20")
            .warnings_into_errors(true)
            .define("WIN32_LEAN_AND_MEAN", None)
            .define("NOMINMAX", None)
            .include("../../sdk/game/include")
            .include("../graphics/include")
            .files(sources)
            .compile("rsf_orchestrator_native");
    }
}
