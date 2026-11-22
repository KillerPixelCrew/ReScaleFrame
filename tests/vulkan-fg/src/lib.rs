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
