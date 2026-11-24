// SPDX-License-Identifier: MIT
//! Fallout 4 AE renderer layout. The globals are discovered (discovery.rs); the field offsets
//! are documented constants, fixed where the discovered patterns address them: the code after
//! the creation call reads the device (+48), context (+50) and window (+58) and stores the
//! context in the thread's render-context block (+B18), and the render-context reads load the
//! block's current context (+B20). The
//! chain (+70) is the swapchain output the creation call passes. See games/fallout4/engine.json.

use crate::discovery::{Site, rva};

pub(crate) const RENDERER_DEVICE: usize = 0x48;
pub(crate) const RENDERER_CONTEXT: usize = 0x50;
pub(crate) const RENDERER_WINDOW: usize = 0x58;
pub(crate) const RENDERER_CHAIN: usize = 0x70;

/// The global renderer owner pointer of the image at `base`.
pub(crate) fn renderer_pointer(base: usize) -> usize {
    base + rva(Site::RendererPointer)
}

/// The renderer object the owner pointer names once the renderer exists.
pub(crate) fn renderer_object(base: usize) -> usize {
    base + rva(Site::RendererObject)
}

/// Obtain a fresh render context on the calling thread. Depth sampling also
/// reads the device context before following the native global fallback.
pub(crate) fn render_context(base: usize, with_device: bool) -> Option<(usize, Option<usize>)> {
    use crate::read;

    let index = read::<u32>(base + rva(Site::TlsIndex)).filter(|&index| index <= 4096)?;
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
        read::<usize>(base + rva(Site::RenderContext))?
    } else {
        context
    };
    Some((context, device))
}
