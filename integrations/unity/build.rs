// SPDX-License-Identifier: MIT
//! Binds the Unity bridge and package headers.

fn main() {
    let headers = [
        "integrations/unity/include/rescaleframe/unity_bridge.h",
        "integrations/unity/include/rescaleframe/unity_runtime_host.h",
        "integrations/unity/native/bridge.h",
        "integrations/unity/native/bridge_d3d12.h",
        "integrations/unity/native/bridge_state.h",
        "integrations/unity/native/package_state.h",
        "integrations/unity/native/vulkan_foreign.h",
    ];
    let builder = rsf_bindgen::owner(
        &headers,
        &["integrations/unity/include"],
        &["integrations/unity"],
        &[],
        &[],
    )
    // The bridge defines the rsf_unity_* calls in Rust; only the engine-side Vulkan entry points
    // of vulkan_foreign.h are declarations it needs.
    .blocklist_function("rsf_unity_.*");
    rsf_bindgen::write(builder, "sys.rs");
}
