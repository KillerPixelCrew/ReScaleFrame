// SPDX-License-Identifier: GPL-3.0-only
// A deterministic panel for the native host test. The real egui DLL can also run that test.
#include <rescaleframe/overlay.h>

struct rsf_overlay {
    bool uploaded = false;
};

extern "C" __declspec(dllexport) rsf_overlay* rsf_overlay_create(uint32_t abi)
{
    return abi == RSF_OVERLAY_ABI_VERSION ? new rsf_overlay() : nullptr;
}

extern "C" __declspec(dllexport) void rsf_overlay_destroy(rsf_overlay* panel)
{
    delete panel;
}

extern "C" __declspec(dllexport) rsf_overlay_result rsf_overlay_frame(
    rsf_overlay*, const rsf_overlay_input*, const rsf_overlay_stats*, rsf_overlay_draw_data* data,
    rsf_overlay_intent*)
{
    static const rsf_overlay_vertex vertices[] = {
        {8, 8, 0, 0, 0xff00ff00u}, {72, 8, 1, 0, 0xff00ff00u},
        {8, 72, 0, 1, 0xff00ff00u},
    };
    static const uint32_t indices[] = {0, 1, 2};
    static const rsf_overlay_draw_call call{0, 3, 0, 0, 0, 80, 80, 0};
    *data = {sizeof(*data), vertices, 3, indices, 3, &call, 1};
    return RSF_OVERLAY_OK;
}

extern "C" __declspec(dllexport) uint32_t rsf_overlay_texture_updates(
    rsf_overlay* panel, rsf_overlay_texture_update* updates, uint32_t capacity)
{
    if (panel->uploaded || capacity == 0) {
        return 0;
    }
    static const uint8_t white[] = {255, 255, 255, 255};
    updates[0] = {0, 0, 0, 1, 1, white, 1};
    panel->uploaded = true;
    return 1;
}

extern "C" __declspec(dllexport) uint32_t rsf_overlay_textures_to_free(
    rsf_overlay*, uint64_t*, uint32_t)
{
    return 0;
}
