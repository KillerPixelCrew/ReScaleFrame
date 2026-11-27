// SPDX-License-Identifier: MIT
//! Binds the Godot integration's private header and the Runtime CPU service it binds at graphics
//! setup. The `rsf_*` Runtime records, the Vulkan host's among them, come from rsf-game-sys.

fn main() {
    let headers = [
        "integrations/godot/native/api.h",
        "runtime/orchestrator/include/rescaleframe/native_cpu_service.h",
    ];
    let builder = rsf_bindgen::owner(
        &headers,
        &[],
        &[
            "integrations/godot/native",
            "runtime/orchestrator/include/rescaleframe/native_cpu_service.h",
        ],
        &[],
        &[],
    );
    rsf_bindgen::write(builder, "sys.rs");
}
