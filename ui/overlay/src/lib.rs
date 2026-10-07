//! GPU-independent egui settings panel and performance HUD behind `overlay.h`.
//!
//! [`Overlay`] owns widget state and converts [`FrameInput`] and [`Stats`] into an [`Intent`],
//! meshes and texture patches. [`ffi`] validates the C boundary and contains unwinding panics.
//! Native renderers own GPU uploads, drawing and restoration of the game's pipeline state.
//! Rates use observed host counters and QPC samples; requested generation is not a rate sample.

#![deny(missing_docs)]

pub mod abi;
pub mod ffi;
pub mod model;
mod overlay;
mod panel;
mod performance;

pub use model::{FrameInput, Intent, Quality, Stats, TextureUpdate};
pub use overlay::Overlay;
pub use panel::Controls;
