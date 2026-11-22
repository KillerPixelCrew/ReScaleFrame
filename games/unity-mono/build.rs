// SPDX-License-Identifier: MIT
//! Binds the Unity bridge and package headers.

fn main() {
    let headers = [
        "games/unity-mono/include/rescaleframe/unity_bridge.h",
        "games/unity-mono/include/rescaleframe/unity_runtime_host.h",
        "games/unity-mono/src/bridge.h",
        "games/unity-mono/src/bridge_d3d12.h",
        "games/unity-mono/src/bridge_state.h",
        "games/unity-mono/src/vulkan_state.h",
        "integrations/unity/native/package_state.h",
        "integrations/unity/native/vulkan_foreign.h",
    ];
    let builder = rsf_bindgen::owner(
        &headers,
        &["games/unity-mono/include"],
        &["games/unity-mono", "integrations/unity"],
        &[],
        &[],
    )
    // The bridge defines the rsf_unity_* calls in Rust; only the engine-side Vulkan entry points
    // of vulkan_foreign.h are declarations it needs.
    .blocklist_function("rsf_unity_.*");
    rsf_bindgen::write(builder, "sys.rs");
}
