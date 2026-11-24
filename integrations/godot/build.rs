// SPDX-License-Identifier: MIT
//! Binds the Godot integration's private headers and the Runtime CPU service it binds at graphics
//! setup. The Vulkan records are `ash::vk`; the `rsf_*` provider records come from rsf-game-sys
//! and the private FSR4 probe header.

fn main() {
    let headers = [
        "integrations/godot/native/api.h",
        "integrations/godot/native/vulkan_features.h",
        "runtime/orchestrator/include/rescaleframe/native_cpu_service.h",
    ];
    let builder = rsf_bindgen::owner(
        &headers,
        &[],
        &[
            "integrations/godot/native",
            "runtime/orchestrator/src/native_fg_fsr4_vulkan.h",
            "runtime/vulkan/fsr4-fg/bridge_api.h",
            "runtime/orchestrator/include/rescaleframe/native_cpu_service.h",
        ],
        &[],
        &[],
    );
    rsf_bindgen::write(builder, "sys.rs");
}
