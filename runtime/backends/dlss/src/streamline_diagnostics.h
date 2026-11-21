/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/backend.h>

enum : uint32_t {
    RSF_STREAMLINE_LOG_LEGACY_SR = 0,
    RSF_STREAMLINE_LOG_D3D12_HOST = 1,
};

using RsfStreamlineEmitLog = void (*)(rsf_backend_log_fn, void*, const char*);

extern "C" {
void rsf_core_streamline_log_set(uint32_t slot, uintptr_t owner, rsf_backend_log_fn, void* user,
    uint32_t verbose);
void rsf_core_streamline_log_clear(uint32_t slot, uintptr_t owner);
void rsf_core_streamline_log_emit(uint32_t slot, uint32_t important, const char* text,
    RsfStreamlineEmitLog emit);
}

namespace rsf {
inline void emit_streamline_log(rsf_backend_log_fn log, void* user, const char* text) noexcept try
{
    if (log) log(user, text);
} catch (...) {}
}
