//! The in-game overlay: an egui panel, and the C boundary the renderer calls it through.
//!
//! The crate is two layers on purpose.
//!
//! [`Overlay`] is the interesting one and is ordinary safe Rust. It owns the widget state, decides
//! what the panel says, and turns egui's output into flat buffers of vertices, indices, draw calls
//! and texture patches. It takes a [`FrameInput`] and a [`Stats`] and hands back an [`Intent`],
//! none of which is a pointer, so all of it can be laid out, clicked and asserted on in a unit
//! test with no device and no game.
//!
//! [`ffi`] is the thin one. It implements exactly the functions in
//! `include/rescaleframe/overlay.h`, checks pointers and struct sizes, and stops panics at the
//! boundary. It contains no layout decisions.
//!
//! What this crate never does is touch a GPU. The D3D11 renderer on the other side of the header
//! owns the device, the pipeline state, and the job of leaving the game's frame exactly as it
//! found it. Drawing inside somebody else's frame is C++ work against a device the game owns;
//! deciding what a quality dropdown does is not.
//!
//! Honesty, since this is a status panel: it shows what it was told, plus ratios derived from it.
//! There is no frame rate gain, no latency figure and no quality score, because the project cannot
//! measure any of those yet.

#![deny(missing_docs)]

pub mod abi;
pub mod ffi;
pub mod model;
mod overlay;
mod panel;

pub use model::{FrameInput, Intent, Quality, Stats, TextureUpdate};
pub use overlay::Overlay;
pub use panel::Controls;
