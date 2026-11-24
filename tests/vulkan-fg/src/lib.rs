// SPDX-License-Identifier: MIT
#![cfg(windows)]

/// The Runtime's C bindings. Every user is behind a fixture feature, hence unused_imports.
#[allow(unused_imports)]
mod sys {
    pub use rsf_orchestrator::sys::*;
}

#[cfg(feature = "xess-input-range")]
mod xess_input_range;
#[cfg(feature = "xess-device")]
mod xess_interop;
#[cfg(feature = "xess-device")]
mod xess_present;
#[cfg(feature = "xess-device")]
mod xess_present_gpu;
#[cfg(feature = "xess-sr")]
mod xess_sr;
#[cfg(feature = "xess-sr")]
mod xess_sr_gpu;
