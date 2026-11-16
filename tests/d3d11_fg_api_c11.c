/* SPDX-License-Identifier: MIT */
/* Compile and run as C11, with a dllimport reference to the actual Runtime DLL. */
#include <rescaleframe/d3d11_fg_api.h>
#include <stddef.h>
#include <stdio.h>

#ifdef RSF_RUNTIME_BUILD
#error The C11 client must import Runtime, never embed its owner.
#endif

_Static_assert(offsetof(rsf_d3d11_fg_api, struct_size) == 0, "size prefix");
_Static_assert(offsetof(rsf_d3d11_fg_api, abi_version) == sizeof(uint32_t), "version prefix");
_Static_assert(offsetof(rsf_d3d11_fg_api, set_log) == 8, "first function offset");

_Static_assert(sizeof(rsf_game_depth_domain) == 104, "copied depth domain layout");
_Static_assert(sizeof(rsf_game_depth_domains) == 240, "copied depth descriptor layout");
_Static_assert(offsetof(rsf_game_render_pass, depth_domains) == offsetof(rsf_game_render_pass, temporal_mask_weights) + 16,
    "append-only depth facts follow ABI22 tail");
_Static_assert(RSF_GAME_ABI_VERSION == 23, "render pass consumer negotiation");
_Static_assert(RSF_D3D11_FG_API_VERSION == 3, "FG table negotiates new pass");

int main(void) {
    rsf_d3d11_fg_api api = {0};
    rsf_get_d3d11_fg_api_fn query = &rsf_get_d3d11_fg_api;
    api.struct_size = sizeof(api);
    api.abi_version = RSF_D3D11_FG_API_VERSION;
    if (query(&api) != RSF_OK || api.struct_size != sizeof(api) || api.abi_version != RSF_D3D11_FG_API_VERSION) return 1;
    if (!api.set_log || !api.options || !api.status || !api.reflex_ordering || !api.cpu || !api.pass ||
        !api.bind_source || !api.capture || !api.inputs_ready || !api.dump_inputs || !api.prepare ||
        !api.retire || !api.present_event || !api.latency_event) return 1;
    puts("PASS: C11 public header and real Runtime DLL-import FG query");
    return 0;
}
