// SPDX-License-Identifier: MIT
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
        "rsf_presentation",
        &["../presentation/src/d3d11_present_bridge.c"],
        &["RSF_STREAMLINE_HOST_BUILD"],
    );
}

/// Binds the private Runtime headers that only this crate and its test crates read. The public
/// headers come from rsf-game-sys, the FSR 4 ones from rsf-fsr4-compat, the Streamline records
/// from rsf-streamline-vk and the frame-tap platform header from rsf-graphics.
fn bindings() {
    const HEADERS: &[&str] = &[
        "runtime/orchestrator/src/d3d11_host_foreign.h",
        "runtime/orchestrator/src/native_fg_xess_vulkan.h",
        "runtime/orchestrator/src/render_host_foreign.h",
        "runtime/graphics/src/sharpening/d3d11.h",
        "runtime/vulkan/fsr4-fg/bridge_api.h",
        // Compiled into this crate through tests/graphics_fixture.rs.
        "tests/native_sr_fsr4_vulkan_fixture.h",
    ];

    let builder = rsf_bindgen::owner(
        HEADERS,
        &[],
        &[
            "runtime/orchestrator/src",
            "runtime/graphics/src",
            "runtime/backends/common",
            "runtime/vulkan/fsr4-fg",
            "tests/native_sr_fsr4_vulkan_fixture.h",
        ],
        &[
            "runtime/backends/fsr/src",
            "runtime/vulkan/fsr4",
            "runtime/vulkan/streamline",
            // rsf-graphics owns the frame-tap platform header.
            "runtime/graphics/src/frame_tap_platform.h",
        ],
        &[],
    )
    // The Vulkan FSR 4 owner stays an untyped handle on the Rust side, as before.
    // Declared by hand where they are called, with Rust reference or handle types.
    .blocklist_function("RsfFsr4Fg.*|rsf_sharpening_requested")
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
    println!("cargo:rerun-if-changed=src/plugin_callbacks.c");
    println!("cargo:rerun-if-changed=../../sdk/game/include");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        sharpening_payload();
        plugin_callbacks();
        native_leaves();
    }
}
