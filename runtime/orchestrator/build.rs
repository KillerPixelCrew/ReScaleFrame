// SPDX-License-Identifier: MIT
use std::path::PathBuf;
use std::process::Command;

/// A C11 build without the default C runtime library, as every C leaf here needs. MSVC takes its
/// flags directly; the others take `-std=c11`.
fn c11() -> cc::Build {
    let mut build = cc::Build::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.flags(["/Zl", "/std:c11"]);
    } else {
        build.std("c11");
    }
    build
}
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
    let mut build = c11();
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
        "pass_payload.c",
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
    let mut build = c11();
    build
        .include("include")
        .warnings_into_errors(true)
        .file(format!("{directory}/pass_payload.c"))
        .compile("rsf_sharpening_payload");
}

fn plugin_callbacks() {
    let mut build = c11();
    build
        .include("../../sdk/game/include")
        .warnings_into_errors(true)
        .file("src/plugin_callbacks.c")
        .compile("rsf_plugin_callbacks");
}

// The native leaves CMake links beside the Runtime archive (rsf_runtime_leaves and the FrameDump,
// Graphics, Presentation, FSR and XeSS C sources they call), for the executables Cargo links (unit
// tests). Each group is linked without bundling, so the staticlib CMake links never carries it:
// CMake builds the same files as its own libraries. Defines follow each CMake target; the vendor
// SDK headers are not used, which compiles the same NOT_COMPILED paths as a checkout without them.
fn native_leaf(name: &str, files: &[&str], defines: &[&str]) {
    let minhook = std::env::var("DEP_MINHOOK_INCLUDE").expect("rsf-minhook-sys include directory");
    let mut build = c11();
    for define in defines {
        build.define(define, None);
    }
    build
        .includes([
            "include",
            "src",
            "../graphics/include",
            "../graphics/src",
            "../presentation/include",
            "../presentation/src",
            "../backends/dlss/include",
            "../backends/fsr/include",
            "../backends/xess/include",
            "../contract/include",
            "../common/include",
            "../../sdk/game/include",
            "../../ui/overlay/include",
            minhook.as_str(),
        ])
        .files(files)
        .warnings_into_errors(true)
        .cargo_metadata(false)
        .compile(name);
    for file in files {
        println!("cargo:rerun-if-changed={file}");
    }
    println!("cargo:rustc-link-lib=static:-bundle={name}");
}

fn native_leaves() {
    let out = std::env::var("OUT_DIR").expect("Cargo output directory");
    println!("cargo:rustc-link-search=native={out}");
    native_leaf(
        "rsf_runtime_leaves",
        &[
            "src/fg12_platform.c",
            "src/native_composition_unload.c",
            "src/render_host_platform.c",
            "src/d3d11_host_foreign.c",
            "../graphics/src/observer_exception_guards.c",
        ],
        &["RSF_RUNTIME_BUILD", "RSF_STREAMLINE_HOST_BUILD"],
    );
    native_leaf(
        "rsf_frame_dump",
        &["src/input_dump.c"],
        &["RSF_RUNTIME_BUILD"],
    );
    native_leaf("rsf_graphics", &["../graphics/src/texture_dump.c"], &[]);
    native_leaf(
        "rsf_presentation",
        &["../presentation/src/d3d11_present_bridge.c"],
        &["RSF_STREAMLINE_HOST_BUILD"],
    );
    native_leaf(
        "rsf_fsr",
        &[
            "../backends/fsr/src/fsr_backend.c",
            "../backends/fsr/src/fsr_generation.c",
        ],
        &[],
    );
    native_leaf(
        "rsf_xess",
        &[
            "../backends/xess/src/xess_backend.c",
            "../backends/xess/src/xess_generation.c",
        ],
        &[],
    );
}

// SEH containment for the Streamline call leaves, under the Runtime's own name
// so the standalone lifecycle archive can link beside it.
fn streamline_guard() {
    println!("cargo:rerun-if-changed=../vulkan/streamline/sdk_guard.c");
    let mut build = c11();
    build
        .define("RSF_SLVK_GUARDED", "rsf_core_slvk_guarded")
        .define("RSF_SLVK_GUARD_ONLY", None)
        .warnings_into_errors(true)
        .file("../vulkan/streamline/sdk_guard.c")
        .compile("rsf_core_slvk_guard");
}

/// Binds the private Runtime headers that only this crate and its test crates read. The public
/// headers come from rsf-game-sys, the FSR 4 ones from rsf-fsr4-compat, the Streamline records
/// from rsf-streamline-vk and the D3D11 output and frame-tap platform headers from rsf-graphics.
fn bindings() {
    const HEADERS: &[&str] = &[
        "runtime/orchestrator/src/auxiliary_sr_platform.h",
        "runtime/orchestrator/src/d3d11_host_foreign.h",
        "runtime/orchestrator/src/fg12_platform.h",
        "runtime/orchestrator/src/latency_antilag.h",
        "runtime/orchestrator/src/native_dlss_vulkan_platform.h",
        "runtime/orchestrator/src/native_fg_dlss_vulkan_capture_ledger.h",
        "runtime/orchestrator/src/native_fg_ffx_vulkan.h",
        "runtime/orchestrator/src/native_dx12_vulkan_platform.h",
        "runtime/orchestrator/src/native_fg_cpu_platform.h",
        "runtime/orchestrator/src/native_fg_vulkan_platform.h",
        "runtime/orchestrator/src/native_fg_xess_vulkan.h",
        "runtime/orchestrator/src/native_latency_vulkan_platform.h",
        "runtime/orchestrator/src/native_latency_vulkan_presentation.h",
        "runtime/orchestrator/src/native_sr_fsr4_vulkan_platform.h",
        "runtime/orchestrator/src/render_host_foreign.h",
        "runtime/orchestrator/src/sharpening_vulkan_core.h",
        "runtime/orchestrator/src/vulkan_fg_resources.h",
        "runtime/orchestrator/src/vulkan_fg_copy_abi.h",
        "runtime/orchestrator/src/vulkan_fg_frames.h",
        "runtime/graphics/src/sharpening/d3d11.h",
        "runtime/backends/dlss/src/streamline_generation_records.h",
        "runtime/backends/dlss/src/streamline_sr_records.h",
        "runtime/backends/common/generation12_platform.h",
        "runtime/shader_patch/src/json.h",
        "runtime/vulkan/fsr4-fg/bridge_api.h",
        // Compiled into this crate through tests/graphics_fixture.rs.
        "tests/native_sr_fsr4_vulkan_fixture.h",
    ];
    fn derives(name: &str) -> &'static [&'static str] {
        match name {
            "rsf_vkfg_barrier" | "rsf_vkfg_image_copy" => &["Debug", "PartialEq", "Eq"],
            _ => &[],
        }
    }
    let builder = rsf_bindgen::owner(
        HEADERS,
        &[],
        &[
            "runtime/orchestrator/src",
            "runtime/graphics/src",
            "runtime/backends/dlss/src",
            "runtime/backends/common",
            "runtime/shader_patch/src",
            "runtime/vulkan/fsr4-fg",
            "tests/native_sr_fsr4_vulkan_fixture.h",
        ],
        &[
            "runtime/backends/fsr/src",
            "runtime/vulkan/fsr4",
            "runtime/vulkan/streamline",
            // rsf-graphics owns the D3D11 output and frame-tap platform headers.
            "runtime/orchestrator/src/d3d11_output_platform.h",
            "runtime/graphics/src/frame_tap_platform.h",
        ],
        &[],
    )
    .parse_callbacks(Box::new(rsf_bindgen::Derives(derives)))
    // The Vulkan FSR 4 owner stays an untyped handle on the Rust side, as before.
    // Declared by hand where they are called, with Rust reference or handle types.
    .blocklist_function(
        "RsfFsr4Fg.*|rsf_dx12vk_interop_(image|timeline|destroy|release_guides)|rsf_sharpening_requested",
    )
    .blocklist_type("rsf_fsr4vk")
    .raw_line("pub type rsf_fsr4vk = ::core::ffi::c_void;");
    rsf_bindgen::write(builder, "sys.rs");
}

fn main() {
    bindings();
    // mfg_provider.rs references the version API without dllimport; CMake links version.lib.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=version");
    }
    println!("cargo::rustc-check-cfg=cfg(rsf_streamline_core)");
    println!("cargo:rustc-cfg=rsf_streamline_core");
    println!("cargo:rerun-if-changed=src/plugin_callbacks.c");
    println!("cargo:rerun-if-changed=../../sdk/game/include");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        shader_patch_parser();
        sharpening_payload();
        plugin_callbacks();
        streamline_guard();
        native_leaves();
    }
}
