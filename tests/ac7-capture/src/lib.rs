// SPDX-License-Identifier: MIT
#![cfg(windows)]
// The fixture executable gets AC7 motion capture and the observer from its CMake archives; unit
// tests link the crates directly.
#[cfg(test)]
use {rsf_ac7 as _, rsf_orchestrator as _, rsf_runtime_common as _};
mod warp;
