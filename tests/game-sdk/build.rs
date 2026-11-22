// SPDX-License-Identifier: MIT
/// Binds the game SDK fixtures' own headers. The Runtime, Anomaly and Unity records they build on
/// come from those crates.
fn bindings() {
    let headers = [
        "tests/unity_sr_d3d12_abi.h",
        "tests/game-sdk/render_host_fixture_abi.h",
    ];
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
    bindings();
}
