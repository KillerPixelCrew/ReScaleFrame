// SPDX-License-Identifier: MIT
#![cfg(windows)]
// This archive carries AC7 motion capture and the Runtime's observer.
use {rsf_ac7 as _, rsf_orchestrator as _};

// The common helpers come from their CMake archive; unit tests link the crate directly.
#[cfg(test)]
use rsf_runtime_common as _;
mod warp;
