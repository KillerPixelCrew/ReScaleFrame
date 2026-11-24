// SPDX-License-Identifier: MIT
//! The translucent velocity pass's D3D11 renderer (`d3d11.rs`: stream-output capture, model and
//! view history, motion and depth submission) and its replay entry points (`replay.rs`), which
//! own the renderer for the current D3D11 device, as the former `std::unique_ptr<Renderer>` did.

#[cfg(windows)]
mod d3d11;
#[cfg(windows)]
pub(crate) mod replay;
