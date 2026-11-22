// SPDX-License-Identifier: MIT
//! Fallout 4 AE 1.11.240 renderer layout. See games/fallout4/engine.json.

pub(crate) const RENDERER_POINTER: usize = 0x38e13a0;
pub(crate) const RENDERER_OBJECT: usize = 0x3a25d10;

pub(crate) const RENDERER_DEVICE: usize = 0x48;
pub(crate) const RENDERER_CONTEXT: usize = 0x50;
pub(crate) const RENDERER_WINDOW: usize = 0x58;
pub(crate) const RENDERER_CHAIN: usize = 0x70;
