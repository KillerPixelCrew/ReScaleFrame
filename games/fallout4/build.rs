// SPDX-License-Identifier: MIT
//! Binds the Fallout 4 adapter's private headers.

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

fn main() {
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
