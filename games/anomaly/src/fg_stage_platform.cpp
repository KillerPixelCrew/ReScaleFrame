// SPDX-License-Identifier: MIT
#include "fg_stage.h"
#include "fg_stage_platform.h"
#include "native_memory.h"
#include <rescaleframe/d3d11_state.h>
#include <windows.h>
#include <cstddef>

namespace rsf::anomaly::fg {
namespace {
// The caller passes HW.pBaseZB from the exact executable profile the carrier selected.
void* engine_depth_view(uint32_t rva) noexcept {
    const auto* image = reinterpret_cast<const std::byte*>(GetModuleHandleW(nullptr));
    void* view = nullptr;
    if (!image || !rva || !guarded_copy(&view, image + rva, sizeof(view))) return nullptr;
    return view;
}

const rsf_anomaly_fg_platform& platform() noexcept {
    static const rsf_anomaly_fg_platform value{
        sizeof(rsf_anomaly_fg_platform),
        // Metadata and producer receipts come directly from the Rust owners.
        // The private table layout stays fixed for existing ABI consumers.
        nullptr, nullptr, 0.0f,
        &engine_depth_view,
        nullptr,
        nullptr,
        {rsf_d3d11_state_save, rsf_d3d11_state_restore},
    };
    return value;
}
}

extern "C" bool rsf_anomaly_fg_configure_native(const wchar_t* ini, bool apply, void* runtime, uint32_t depth_rva,
    rsf_game_log_fn log) noexcept {
    return rsf_anomaly_fg_configure_ini(ini, apply, runtime, depth_rva, log, &platform());
}
}
