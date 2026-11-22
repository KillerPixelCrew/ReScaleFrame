// SPDX-License-Identifier: MIT
//! Lifecycle owner for the Streamline Vulkan interposer.
//!
//! `runtime/vulkan/streamline/bridge.cpp` keeps the `VulkanStorySl*` exports, the
//! typed SDK entry points, the slInit preferences and the verified-file leaf. This
//! crate decides when the interposer is admitted, loaded, initialized, shut down
//! and unloaded, which features slInit loads, and which Vulkan dispatch entry
//! points stay usable after SDK shutdown. It also owns the legacy bridge's
//! DLSS-G options cache, resource state and current borrowed frame token.
//!
//! Only Windows links the loader bindings and C entry points. Other hosts build
//! the portable state machine for its synthetic tests and an empty archive
//! otherwise, so the Cargo workspace stays linkable there.

#[cfg(any(windows, test))]
mod diagnostics;
#[cfg(windows)]
mod frame_generation;
#[cfg(windows)]
mod frame_token;
#[cfg(any(windows, test))]
mod lifecycle;
#[cfg(any(windows, test))]
mod sdk_records;
#[cfg(any(windows, test))]
mod sdk_records_abi;
#[cfg(windows)]
mod win32;
