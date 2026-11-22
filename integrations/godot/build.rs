// SPDX-License-Identifier: MIT
//! Binds the Godot integration's private headers. The Godot Vulkan creation records stay in the
//! committed native/vulkan/creation/abi.rs, generated from the pinned engine headers.

fn main() {
    let headers = [
        "integrations/godot/native/extension_abi.h",
        "integrations/godot/native/host_abi.h",
        "integrations/godot/native/vulkan_features.h",
    ];
    let builder = rsf_bindgen::owner(
        &headers,
        &[],
        &["integrations/godot/native"],
        &["runtime/vulkan/fsr4-fg"],
        &[],
    )
    // A Rust record the header only forward-declares.
    .blocklist_type("rsf_godot_effect_readback")
    .raw_line("pub(crate) use crate::platform::EffectReadback as rsf_godot_effect_readback;")
    // The Vulkan code builds on the creation records, generated with the engine's Vulkan
    // headers; these names refer to them instead of to rsf-game-sys's copies.
    .raw_line(
        "pub(crate) use crate::vulkan::creation::abi::{rsf_fsr4fg_info, rsf_input_dump_vk_setup, \
             rsf_srvk_image, rsf_srvk_setup};",
    )
    .raw_line("pub(crate) type rsf_input_dump_vk = ::core::ffi::c_void;");
    rsf_bindgen::write(builder, "sys.rs");
}
