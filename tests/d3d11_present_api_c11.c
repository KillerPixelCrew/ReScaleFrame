/* SPDX-License-Identifier: GPL-3.0-only */
/* Compile and run as C11, with a dllimport reference to the actual Runtime DLL. */
#include <rescaleframe/d3d11_present_api.h>
#include <stddef.h>
#include <stdio.h>

#ifdef RSF_RUNTIME_BUILD
#error The C11 client must import Runtime, never embed its owner.
#endif

_Static_assert(offsetof(rsf_d3d11_present_api, struct_size) == 0, "size prefix");
_Static_assert(offsetof(rsf_d3d11_present_api, abi_version) == sizeof(uint32_t), "version prefix");
_Static_assert(offsetof(rsf_d3d11_present_api, install) == 8, "first function offset");

int main(void) {
    rsf_d3d11_present_api api = {0};
    rsf_get_d3d11_present_api_fn query = &rsf_get_d3d11_present_api;
    api.struct_size = sizeof(api);
    api.abi_version = RSF_D3D11_PRESENT_API_VERSION;
    if (query(&api) != RSF_OK || api.struct_size != sizeof(api) ||
        api.abi_version != RSF_D3D11_PRESENT_API_VERSION) return 1;
    if (!api.install || !api.prepare || !api.create_for_hwnd || !api.host || !api.graphics ||
        !api.queue || !api.command_list || !api.retire || !api.is_owner || !api.has_owner ||
        !api.provider || !api.session || !api.backend || !api.backend_choices || !api.request ||
        !api.switch_result || !api.generation || !api.begin || !api.marker || !api.acquire ||
        !api.input || !api.abort) return 1;
    puts("PASS: C11 public header and real Runtime DLL-import query");
    return 0;
}
