// SPDX-License-Identifier: MIT
//! Compiles the C11 leaves for the executables Cargo links (unit tests). The archive is linked
//! without bundling, so the staticlib CMake links never carries it: the adapter DLL compiles the
//! same files itself.
const LEAVES: [&str; 7] = [
    "src/host_boundary.c",
    "src/cpu_events.c",
    "src/render_inputs.c",
    "src/render_hooks.c",
    "src/resolution.c",
    "src/screen.c",
    "src/depth_domains.c",
];

fn main() {
    bindings();
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=src");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let minhook = std::env::var("DEP_MINHOOK_INCLUDE").expect("rsf-minhook-sys include directory");
    let mut build = cc::Build::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.flags(["/Zl", "/std:c11"]);
    }
    build
        .includes([
            "src",
            minhook.as_str(),
            "../../sdk/game/include",
            "../../runtime/common/include",
            "../../runtime/orchestrator/include",
            "../../runtime/graphics/include",
            "../../runtime/presentation/include",
        ])
        .files(LEAVES)
        .warnings_into_errors(true)
        .cargo_metadata(false)
        .compile("rsf_fallout4_native");
    let out = std::env::var("OUT_DIR").expect("Cargo output directory");
    println!("cargo::rustc-link-search=native={out}");
    println!("cargo::rustc-link-lib=static:-bundle=rsf_fallout4_native");
}

/// The comparisons the render hooks and their tests make on the records.
fn derives(name: &str) -> &'static [&'static str] {
    match name {
        "NativeResolutionReceipt"
        | "NativeAuxiliaryInputs"
        | "NativeDepthMathFailure"
        | "NativeDepthCamera"
        | "NativeDepthMaps"
        | "NativeInputState"
        | "NativeCamera"
        | "CameraRaster"
        | "NativeInputReceipt"
        | "NativeInputHistoryReceipt"
        | "NativeFramebufferReceipt" => &["PartialEq"],
        "NativeScreen"
        | "NativeInputReferences"
        | "NativeRenderExports"
        | "NativeRecoveryLease"
        | "NativePublicationInputs"
        | "NativeOutputOwner"
        | "NativeInputSwapFacts"
        | "NativeInputResourceFacts"
        | "NativeWorldAdmission"
        | "NativeRenderScopeKey"
        | "NativeRenderOwner"
        | "NativeEffectsCall" => &["PartialEq", "Eq"],
        "rsf_fallout4_cpu_identity" => &["Debug", "PartialEq", "Eq"],
        _ => &[],
    }
}

/// Binds the Fallout 4 adapter's private headers.
fn bindings() {
    let headers = [
        "games/fallout4/src/cpu_events_platform.h",
        "games/fallout4/src/fallout4_platform.h",
        "games/fallout4/src/render_hooks_platform.h",
        "games/fallout4/src/resolution_stages.h",
    ];
    let builder = rsf_bindgen::owner(&headers, &[], &["games/fallout4"], &[], &[])
        .parse_callbacks(Box::new(rsf_bindgen::Derives(derives)))
        // The adapter declares the C calls it makes by hand.
        .ignore_functions();
    rsf_bindgen::write(builder, "sys.rs");
}
