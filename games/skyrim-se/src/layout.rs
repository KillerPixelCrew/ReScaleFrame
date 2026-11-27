// SPDX-License-Identifier: MIT
//! Native-creation layout of Steam `SkyrimSE.exe` and the SSE Display Tweaks relay that may own its
//! creation call. Game and Display Tweaks sites are discovered (`discovery.rs`); what remains here
//! are field offsets and the relay's shape. Evidence:
//! docs/research/skyrim-se-integration.md, docs/research/skyrim-display-tweaks-creation-20261116.md
//! and docs/research/skyrim-steam-present-continuation-20261116.md.

use crate::discovery::{self, Site, rva};

/// The renderer initializer's `call` of the D3D11CreateDeviceAndSwapChain import thunk
/// (`Site::CreateCall`) is five bytes; the thunk (`Site::CreateThunk`) jumps through the current
/// IAT slot, so any wrapper that replaced the IAT entry still receives the call.
pub(crate) const CREATE_CALL_LENGTH: usize = 5;

/// Global renderer-data owner (`Site::Owner`). Its creation passes `&owner[+96]`, `&owner[+56]`
/// and `&owner[+64]` as the swapchain, device and context outputs and `owner[+72]` as the window.
/// A null owner or null device means no device exists yet (the cold owner).
pub(crate) const OWNER_DEVICE: usize = 56;
pub(crate) const OWNER_CONTEXT: usize = 64;
pub(crate) const OWNER_WINDOW: usize = 72;
pub(crate) const OWNER_SWAPCHAIN: usize = 96;

/// The renderer's 0x30-byte render-target slot `index`.
pub(crate) fn target_slot(base: usize, index: usize) -> usize {
    base + discovery::target_slots() + index * 0x30
}

pub(crate) fn before_device(base: usize, read: impl Fn(usize) -> Option<usize>) -> Option<bool> {
    let owner = rva(Site::Owner);
    if base == 0 || owner == 0 {
        return None;
    }
    cold_owner(base.checked_add(owner)?, read)
}

/// Whether the owner global at `slot` is null or has no device yet.
fn cold_owner(slot: usize, read: impl Fn(usize) -> Option<usize>) -> Option<bool> {
    let owner = read(slot)?;
    Some(owner == 0 || read(owner.checked_add(OWNER_DEVICE)?)? == 0)
}

/// Second supported creation route: SSE Display Tweaks rewrites only the E8 displacement of the
/// creation call to an SKSE relay `jmp [rip+0]` that lands on its pre/post wrapper, which calls
/// the unchanged game thunk through a retained slot. The wrapper, that call and the slot are
/// discovered in the loaded module (`discovery::display_tweaks`).
pub(crate) const DISPLAY_TWEAKS_MODULE: &str = "SSEDisplayTweaks.dll";
pub(crate) const RELAY_JUMP: [u8; 6] = [0xff, 0x25, 0x00, 0x00, 0x00, 0x00];
pub(crate) const RELAY_LENGTH: usize = RELAY_JUMP.len() + size_of::<u64>();
/// Target of a rel32 call or jump whose next instruction starts at `next`.
pub(crate) fn rel32_target(next: usize, displacement: [u8; 4]) -> usize {
    next.wrapping_add_signed(i32::from_le_bytes(displacement) as isize)
}

#[cfg(test)]
mod tests;
