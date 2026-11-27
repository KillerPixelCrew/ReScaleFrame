/* SPDX-License-Identifier: MIT */
#include <rescaleframe/overlay_d3d12.h>
#include <stddef.h>

typedef int (*overlay_present_fn)(void*, void*, void*, const rsf_overlay_stats*, rsf_overlay_intent*);
int rsf_test_overlay_native12(overlay_present_fn draw);

static int draw(void* device, void* queue, void* chain, const rsf_overlay_stats* stats,
    rsf_overlay_intent* intent)
{
    return rsf_overlay_native12_present(device, queue, chain, stats, intent, NULL, NULL);
}

int rsf_test_overlay_native12_run(void)
{
    return rsf_test_overlay_native12(draw);
}
