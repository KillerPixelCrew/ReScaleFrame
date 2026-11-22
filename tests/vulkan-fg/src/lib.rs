// SPDX-License-Identifier: MIT
#![cfg(windows)]

#[cfg(feature = "dispatch")]
mod dispatch;
#[cfg(feature = "dispatch")]
mod dispatch_abi;
#[cfg(feature = "ffx")]
mod ffx;
#[cfg(feature = "ffx")]
mod ffx_abi;
#[cfg(feature = "fsr4-state")]
mod fsr4_state;
#[cfg(feature = "fsr4-state")]
#[allow(dead_code)]
mod fsr4_state_abi;
mod wsi;
mod wsi_abi;

#[cfg(feature = "xess-device")]
#[allow(dead_code)]
#[path = "../../../runtime/orchestrator/src/vulkan_sr_abi.rs"]
mod vulkan_sr_abi;
#[cfg(feature = "xess-device")]
mod xess_interop;

#[cfg(feature = "xess-device")]
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
