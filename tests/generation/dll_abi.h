/* SPDX-License-Identifier: MIT */
#include <rescaleframe/d3d11_fg_api.h>
#include <rescaleframe/d3d11_present_api.h>
#include <rescaleframe/native_cpu.h>

typedef struct rsf_fg_fixture_imports {
    rsf_get_d3d11_fg_api_fn fg;
    rsf_get_d3d11_present_api_fn present;
    int (*read_cpu)(uint64_t session, uint64_t source, rsf_native_cpu_frame*);
    void (*render_latency)(uint64_t source, uint32_t begin);
} rsf_fg_fixture_imports;
