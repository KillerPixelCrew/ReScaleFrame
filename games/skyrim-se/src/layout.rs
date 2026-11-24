// SPDX-License-Identifier: MIT
//! Native-creation layout of Steam `SkyrimSE.exe` and the exact SSE Display Tweaks 0.5.25 relay
//! that may own its creation call. Game sites are discovered (`discovery.rs`); what remains here
//! are field offsets and the Display Tweaks module profile. Evidence:
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

/// Second supported creation route: Display Tweaks 0.5.25 rewrites only the E8 displacement of
/// the creation call to an SKSE relay `jmp [rip+0]` that lands on its pre/post wrapper, which calls the
/// unchanged game thunk through a retained slot.
pub(crate) const DISPLAY_TWEAKS_MODULE: &str = "SSEDisplayTweaks.dll";
pub(crate) const DISPLAY_TWEAKS_SHA256: &[u8; 64] =
    b"df7f352a3f13736709c5f9d265ebf311d9d77d373a9a1b8cbe2aee7fdd8ba9f4";
pub(crate) const RELAY_JUMP: [u8; 6] = [0xff, 0x25, 0x00, 0x00, 0x00, 0x00];
pub(crate) const RELAY_LENGTH: usize = RELAY_JUMP.len() + size_of::<u64>();
/// Display Tweaks module RVAs: wrapper entry, its call of the retained thunk, the retained slot
/// and the return address of that call.
pub(crate) const DISPLAY_ENTRY: usize = 0x2_7250;
pub(crate) const DISPLAY_PROLOGUE: [u8; 20] = [
    0x40, 0x53, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x48, 0x81, 0xec,
    0xc8, 0x01, 0x00, 0x00,
];
pub(crate) const DISPLAY_INVOKE: usize = 0x2_72e9;
/// `mov r10,[rip+0x36e78]` (the retained slot) through `call r10; mov ebx,eax`.
pub(crate) const DISPLAY_INVOKE_BYTES: [u8; 77] = [
    0x4c, 0x8b, 0x15, 0x78, 0x6e, 0x03, 0x00, 0x4c, 0x8b, 0xc5, 0x48, 0x89, 0x44, 0x24, 0x58, 0x8b,
    0xd6, 0x8b, 0x84, 0x24, 0x40, 0x02, 0x00, 0x00, 0x48, 0x89, 0x5c, 0x24, 0x50, 0x4c, 0x89, 0x64,
    0x24, 0x48, 0x4c, 0x89, 0x7c, 0x24, 0x40, 0x4c, 0x89, 0x6c, 0x24, 0x38, 0x89, 0x44, 0x24, 0x30,
    0x8b, 0x84, 0x24, 0x38, 0x02, 0x00, 0x00, 0x89, 0x44, 0x24, 0x28, 0x48, 0x89, 0x7c, 0x24, 0x20,
    0x48, 0x8b, 0x7c, 0x24, 0x70, 0x48, 0x8b, 0xcf, 0x41, 0xff, 0xd2, 0x8b, 0xd8,
];
pub(crate) const DISPLAY_RETAINED: usize = 0x5_e168;
pub(crate) const DISPLAY_RETURN: usize = 0x2_7334;

/// Target of a rel32 call or jump whose next instruction starts at `next`.
pub(crate) fn rel32_target(next: usize, displacement: [u8; 4]) -> usize {
    next.wrapping_add_signed(i32::from_le_bytes(displacement) as isize)
}

#[cfg(test)]
mod tests;
