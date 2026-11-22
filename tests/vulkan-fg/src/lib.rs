// SPDX-License-Identifier: MIT
#![cfg(windows)]

#[cfg(feature = "dispatch")]
mod dispatch;
#[cfg(feature = "dispatch")]
mod dispatch_abi;
#[cfg(feature = "dispatch")]
mod dispatch_drivers;
#[cfg(feature = "dlss")]
mod dlss;
#[cfg(feature = "dlss")]
#[allow(dead_code)]
mod dlss_abi;
#[cfg(feature = "ffx")]
mod ffx;
#[cfg(feature = "ffx")]
mod ffx_abi;
#[cfg(feature = "ffx")]
mod ffx_gpu;
#[cfg(feature = "ffx")]
mod ffx_sdk;
#[cfg(feature = "ffx")]
#[path = "../../../runtime/vulkan/fsr3/sdk_abi.rs"]
mod ffx_sdk_abi;
#[cfg(feature = "fsr4-state")]
mod fsr4_state;
#[cfg(feature = "fsr4-state")]
#[allow(dead_code)]
mod fsr4_state_abi;
#[cfg(any(feature = "streamline-selected", feature = "streamline-dispatch"))]
#[allow(dead_code)]
mod streamline_abi;
#[cfg(feature = "streamline-dispatch")]
mod streamline_dispatch;
#[cfg(feature = "streamline-selected")]
mod streamline_selected;
#[cfg(feature = "wsi-dispatch")]
mod wsi;
#[cfg(feature = "wsi-dispatch")]
mod wsi_abi;

#[cfg(any(
    feature = "xess-device",
    feature = "xess-sr",
    feature = "xess-input-range"
))]
#[allow(dead_code)]
#[path = "../../../runtime/orchestrator/src/vulkan_sr_abi.rs"]
mod vulkan_sr_abi;
#[cfg(feature = "xess-device")]
mod xess_interop;

#[cfg(any(
    feature = "xess-device",
    feature = "xess-sr",
    feature = "xess-input-range"
))]
mod render_abi {
    pub use rsf_game_sys::sharpening_abi::rsf_sharpening_options;
}
#[cfg(feature = "xess-device")]
#[allow(dead_code)]
#[path = "../../../runtime/orchestrator/src/dx12_vulkan_interop_abi.rs"]
mod dx12_vulkan_interop_abi;
#[cfg(feature = "xess-device")]
#[path = "../../../runtime/vulkan/xess-fg/bridge_abi.rs"]
mod xess_bridge_abi;

#[cfg(feature = "xess-device")]
#[allow(dead_code)]
#[path = "../../../runtime/orchestrator/src/vulkan_fg_abi.rs"]
mod vulkan_fg_abi;
#[cfg(feature = "xess-device")]
mod xess_present;

#[cfg(feature = "xess-input-range")]
mod xess_input_range;
#[cfg(any(feature = "xess-sr", feature = "xess-input-range"))]
#[path = "../../../runtime/orchestrator/src/vulkan_sr/xess_requirements_abi.rs"]
mod xess_requirements_abi;
#[cfg(feature = "xess-input-range")]
#[path = "../../../runtime/orchestrator/src/vulkan_sr/xess_sdk_abi.rs"]
mod xess_sdk_abi;
#[cfg(feature = "xess-sr")]
mod xess_sr;

#[cfg(feature = "xess-device")]
mod xess_present_gpu;
#[cfg(feature = "xess-device")]
#[path = "../../../runtime/orchestrator/src/vulkan_xess_fg_abi.rs"]
mod xess_present_options;

#[cfg(feature = "xess-sr")]
mod xess_sr_gpu;
