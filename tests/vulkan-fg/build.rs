// SPDX-License-Identifier: MIT
/// Binds the Vulkan FG fixture headers and the C++-only Runtime platform headers only these
/// fixtures read, in C++ mode as those headers require. The Vulkan WSI dispatch binding stays in
/// the committed src/wsi_abi.rs because its header needs the Vulkan SDK.
fn bindings() {
    let own = [
        "tests/vulkan-fg/dispatch_abi.h",
        "tests/vulkan-fg/ffx_abi.h",
        "tests/vulkan-fg/dlss_abi.h",
        "tests/vulkan-fg/streamline_abi.h",
        "tests/vulkan-fg/results.h",
        "runtime/orchestrator/src/native_fg_fsr4_vulkan_platform.h",
        "runtime/orchestrator/src/native_fg_fsr4_vulkan.h",
        "runtime/orchestrator/src/native_fg_dx12_vulkan_platform.h",
        "runtime/orchestrator/src/native_fg_dlss_vulkan_platform.h",
    ];
    let builder = rsf_bindgen::owner(
        &own[..6],
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
