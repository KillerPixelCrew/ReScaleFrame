// SPDX-License-Identifier: MIT
//! Binds the FSR 4 compatibility, FidelityFX version and Vulkan FSR 4 bridge headers. The
//! orchestrator and the standalone fixtures use these items from this crate.

fn main() {
    let builder = rsf_bindgen::owner(
        &[
            "runtime/backends/fsr/src/fsr4_compat_platform.h",
            "runtime/backends/fsr/src/ffx_version.h",
            "runtime/vulkan/fsr4/bridge_api.h",
        ],
        &[],
        &["runtime/backends/fsr/src", "runtime/vulkan/fsr4"],
        &[],
        &[],
    )
    // The bridge stays an untyped handle on the Rust side, as before.
    .blocklist_type("rsf_vk_fsr4")
    .raw_line("pub type rsf_vk_fsr4 = ::core::ffi::c_void;");
    rsf_bindgen::write(builder, "sys.rs");
}
