// SPDX-License-Identifier: MIT
/// Binds the Vulkan FG fixture headers. The Vulkan WSI dispatch binding stays in the committed
/// src/wsi_abi.rs because its header needs the Vulkan SDK.
fn bindings() {
    let own = [
        "tests/vulkan-fg/ffx_abi.h",
        "tests/vulkan-fg/streamline_abi.h",
        "tests/vulkan-fg/results.h",
    ];
    let builder = rsf_bindgen::owner(
        &own,
        &[],
        &own,
        &[
            "runtime/orchestrator/src",
            "runtime/graphics/src",
            "runtime/backends",
            "runtime/shader_patch/src",
            "runtime/vulkan",
        ],
        &["-x", "c++", "-std=c++17", "-Wno-pragma-once-outside-header"],
    )
    .prepend_enum_name(false)
    // Defined in Rust by this crate.
    .blocklist_function("rsf_fixture_.*_run");
    rsf_bindgen::write(builder, "sys.rs");
}

fn main() {
    bindings();
}
