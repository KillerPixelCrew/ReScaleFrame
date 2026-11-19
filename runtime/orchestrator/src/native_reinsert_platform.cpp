// SPDX-License-Identifier: MIT
#include "d3d11_output_platform.h"
#include "native_reinsert.h"
#include <rescaleframe/fullscreen_pass.h>
#include <rescaleframe/native_composition.h>

extern "C" uint32_t rsf_core_native_reinsert(void* context, void* source,
    void* destination, void* readable, const int32_t rect[4], const rsf_d3d11_reinsert_kernels* kernels);
extern "C" void rsf_core_native_reinsert_release();

extern "C" {
static rsf_fullscreen_pass* create_blit(void* device) try
{
    const rsf_fullscreen_setup setup{sizeof(setup), RSF_FULLSCREEN_PASS_ABI_VERSION, nullptr, nullptr};
    rsf_fullscreen_pass* result = nullptr;
    return rsf_fullscreen_pass_create(device, &setup, &result) == RSF_FULLSCREEN_OK ? result : nullptr;
}
catch (...) { return nullptr; }

static uint32_t draw_rgba(rsf_fullscreen_pass* blit, void* context, void* target,
    void* source, uint32_t width, uint32_t height) try
{
    const rsf_fullscreen_draw draw{sizeof(draw), RSF_FULLSCREEN_COPY_RGBA, 1, width, height};
    return rsf_fullscreen_pass_draw(blit, context, target, source, &draw) == RSF_FULLSCREEN_OK;
}
catch (...) { return 0; }
}

bool rsf_native_reinsert(ID3D11DeviceContext* context, ID3D11Texture2D* source,
    ID3D11Texture2D* destination, ID3D11Texture2D* readable, const int32_t rect[4])
{
    static constexpr rsf_d3d11_reinsert_kernels kernels{
        rsf_d3d11_state_save, rsf_d3d11_state_restore, create_blit, draw_rgba, rsf_fullscreen_pass_destroy};
    return rsf_core_native_reinsert(context, source, destination, readable, rect, &kernels) != 0;
}

void rsf_native_reinsert_release() { rsf_core_native_reinsert_release(); }

namespace {
// Preserve COM retirement at module unload when explicit graphics teardown was skipped.
struct ReleaseAtUnload {
    ~ReleaseAtUnload() {
        rsf_native_composition_release();
        rsf_core_native_reinsert_release();
    }
} release_at_unload;
}
