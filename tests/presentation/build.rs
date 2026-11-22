// SPDX-License-Identifier: MIT
/// Binds the presentation fixtures' own headers. The Runtime and AC7 records they build on come
/// from those crates.
fn bindings() {
    let headers = [
        "tests/presentation/device_fixture_bridge.h",
        "tests/presentation/d3d11_marker_bridge.h",
        "tests/presentation/d3d11_scope_fixture.h",
        "tests/presentation/present_fixture_loops.h",
        "tests/d3d11_host_device_fixture.h",
    ];
    let builder = rsf_bindgen::owner(
        &headers,
        &["games/ac7/include"],
        &["tests/presentation", "tests/d3d11_host_device_fixture.h"],
        &[
            "runtime/orchestrator/src",
            "runtime/graphics/src",
            "runtime/backends",
            "runtime/shader_patch/src",
            "runtime/vulkan",
            "games/ac7",
        ],
        &[],
    )
    // Defined in Rust by this crate.
    .blocklist_function("rsf_(d3d11_fixture|fg_bridge_fixture|fg_fixture)_.*");
    rsf_bindgen::write(builder, "sys.rs");
}

fn main() {
    bindings();
}
