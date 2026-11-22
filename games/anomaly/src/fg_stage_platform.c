/* SPDX-License-Identifier: MIT */
#include "fg_stage.h"
#include "fg_stage_platform.h"
#include "native_memory.h"
#include <rescaleframe/d3d11_state.h>

/* HW.pBaseZB comes from the exact executable profile selected by the carrier. */
static void* engine_depth_view(uint32_t rva)
{
    const unsigned char* image = (const unsigned char*)GetModuleHandleW(NULL);
    void* view = NULL;
    if (!image || !rva || !guarded_copy(&view, image + rva, sizeof(view))) return NULL;
    return view;
}

/* Rust owns metadata and producer receipts. The private layout stays fixed. */
static const rsf_anomaly_fg_platform platform = {
    sizeof(rsf_anomaly_fg_platform),
    NULL, NULL, 0.0f,
    engine_depth_view,
    NULL, NULL,
    {rsf_d3d11_state_save, rsf_d3d11_state_restore},
};

bool rsf_anomaly_fg_configure_native(const wchar_t* ini, bool apply, void* runtime, uint32_t depth_rva,
    rsf_game_log_fn log)
{
    return rsf_anomaly_fg_configure_ini(ini, apply, runtime, depth_rva, log, &platform);
}
