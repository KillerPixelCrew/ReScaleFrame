// SPDX-License-Identifier: MIT
/// Binds the game SDK fixtures' own headers. The Runtime, Anomaly and Unity records they build on
/// come from those crates. The one header is the Unity SR fixture's, which names rsf-unity's records,
/// so other feature sets bind nothing and do not link rsf-unity.
fn bindings() {
    let headers = ["tests/unity_sr_d3d12_abi.h"];
    let builder = rsf_bindgen::owner(
        &headers,
        &["integrations/unity/include"],
        &headers,
        &[
            "runtime/orchestrator/src",
            "runtime/graphics/src",
            "runtime/backends",
            "runtime/shader_patch/src",
            "runtime/vulkan",
            "games/anomaly",
            "integrations/unity",
        ],
        &["-fdeclspec"],
    )
    .prepend_enum_name(false);
    rsf_bindgen::write(builder, "sys.rs");
}

fn main() {
    if std::env::var_os("CARGO_FEATURE_UNITY_SR_D3D12_FIXTURE").is_some() {
        bindings();
    } else {
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
        std::fs::write(out.join("sys.rs"), "").expect("write sys.rs");
    }
}
