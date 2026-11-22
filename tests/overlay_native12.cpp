// SPDX-License-Identifier: MIT
#include <rescaleframe/overlay_d3d12.h>

extern "C" {
using overlay_present_fn = int (*)(void*, void*, void*, const rsf_overlay_stats*, rsf_overlay_intent*);
int rsf_test_overlay_native12(overlay_present_fn draw);

int rsf_test_overlay_native12_run()
{
    // The production D3D12 wrapper has C++ linkage. Exercise that same path.
    return rsf_test_overlay_native12([](void* device, void* queue, void* chain,
        const rsf_overlay_stats* stats, rsf_overlay_intent* intent) {
        return rsf_overlay_native12_present(device, queue, chain, stats, intent, nullptr, nullptr);
    });
}
}
