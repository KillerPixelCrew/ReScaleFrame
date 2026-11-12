// SPDX-License-Identifier: GPL-3.0-only
// Guarded hooks that give the Anomaly overlay its two engine boundaries: the DXGI swap chain
// Present/ResizeBuffers the engine calls (after its last UI draw) and the DirectInput mouse and
// keyboard devices it polls. Both are installed from the dummy objects' vtables, so no engine RVA
// is involved and the engine executable is not modified. Trampolines stay for the process lifetime.
#pragma once
#include "overlay_input_gate.h"
#include <dxgi.h>

namespace rsf::anomaly::overlay {
struct PresentSink {
    // Present-owner thread, immediately before the real Present (never for DXGI_PRESENT_TEST).
    void (*before_present)(void* user, IDXGISwapChain* chain) noexcept;
    // Before ResizeBuffers / ResizeTarget is forwarded.
    void (*resize)(void* user, IDXGISwapChain* chain) noexcept;
    void* user;
};

// Both return false with `reason` (a static string) and install nothing further on refusal.
// One call per process; a second returns false.
bool install_present_hooks(const PresentSink& sink, const char** reason) noexcept;
bool install_input_hooks(InputGate& gate, const char** reason) noexcept;
}
