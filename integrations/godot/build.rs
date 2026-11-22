// SPDX-License-Identifier: MIT
//! Binds the Godot integration's private headers. The Vulkan records are `ash::vk`; the
//! `rsf_*` provider records come from rsf-game-sys and the private FSR4 probe header.

fn main() {
    let headers = [
        "integrations/godot/native/api.h",
        "integrations/godot/native/extension_abi.h",
        "integrations/godot/native/host_abi.h",
        "integrations/godot/native/vulkan_features.h",
    ];
    let builder = rsf_bindgen::owner(
        &headers,
        &[],
        &[
            "integrations/godot/native",
            "runtime/orchestrator/src/native_fg_fsr4_vulkan.h",
            "runtime/vulkan/fsr4-fg/bridge_api.h",
        ],
        &[],
        &[],
    )
    // A Rust record the header only forward-declares.
    .blocklist_type("rsf_godot_effect_readback")
    .raw_line("pub(crate) use crate::platform::EffectReadback as rsf_godot_effect_readback;");
    rsf_bindgen::write(builder, "sys.rs");
}
