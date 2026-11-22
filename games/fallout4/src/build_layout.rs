// SPDX-License-Identifier: MIT
//! Fallout 4 AE 1.11.240 renderer layout. See games/fallout4/engine.json.

pub(crate) const RENDERER_POINTER: usize = 0x38e13a0;
pub(crate) const RENDERER_OBJECT: usize = 0x3a25d10;

pub(crate) const RENDERER_DEVICE: usize = 0x48;
pub(crate) const RENDERER_CONTEXT: usize = 0x50;
pub(crate) const RENDERER_WINDOW: usize = 0x58;
pub(crate) const RENDERER_CHAIN: usize = 0x70;

/// Obtain a fresh render context on the calling thread. Depth sampling also
/// reads the device context before following the native global fallback.
pub(crate) fn render_context(base: usize, with_device: bool) -> Option<(usize, Option<usize>)> {
    use crate::read;

    let index = read::<u32>(base + 0x3e73018).filter(|&index| index <= 4096)?;
    let slots: usize;
    // SAFETY: On Windows x64, GS points to this thread's TEB.
    unsafe {
        std::arch::asm!(
            "mov {}, gs:[0x58]",
            out(reg) slots,
            options(nostack, readonly, preserves_flags),
        );
    }
    if slots == 0 {
        return None;
    }
    let block = read::<usize>(slots.wrapping_add(index as usize * size_of::<usize>()))
        .filter(|&v| v != 0)?;
    let context = read::<usize>(block.wrapping_add(0xb20))?;
    let device = if with_device {
        Some(read::<usize>(block.wrapping_add(0xb18))?)
    } else {
        None
    };
    let context = if context == 0 {
        read::<usize>(base + 0x38e1398)?
    } else {
        context
    };
    Some((context, device))
}
