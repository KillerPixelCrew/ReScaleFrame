// SPDX-License-Identifier: MIT
#include <rescaleframe/overlay_renderer.h>

// Keep foreign exceptions in the native caller's language.
extern "C" void rsf_overlay_renderer_log(rsf_overlay_renderer_log_fn log, void* user,
    const char* message) noexcept
{
    try { log(user, message); } catch (...) {}
}
