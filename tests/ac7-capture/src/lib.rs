// SPDX-License-Identifier: MIT
#![cfg(windows)]
// This archive carries AC7 motion capture and the Runtime's observer; the common helpers come from
// their CMake archive, which unit tests link directly.
use {rsf_ac7 as _, rsf_orchestrator as _};
#[cfg(test)]
use rsf_runtime_common as _;
mod warp;
