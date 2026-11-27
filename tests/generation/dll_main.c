/* SPDX-License-Identifier: MIT */
#include "dll_abi.h"

#ifdef RSF_RUNTIME_BUILD
#error The FG fixture must import the Runtime DLL.
#endif

extern int rsf_test_d3d11_fg_api(const rsf_fg_fixture_imports*, const rsf_d3d11_fg_api*);

int main(void)
{
    const rsf_fg_fixture_imports imports = {
        &rsf_get_d3d11_fg_api,
        &rsf_get_d3d11_present_api,
        &rsf_native_cpu_read,
        &rsf_native_latency_render
    };
    rsf_d3d11_fg_api native = {0};
    /* Resolve these addresses through dllimport so pointer comparisons use the
       DLL addresses rather than any import thunks. */
    native.prepare = &rsf_native_fg_prepare;
    native.retire = &rsf_native_fg_retire;
    native.options = &rsf_native_fg_options_set;
    native.status = &rsf_native_fg_status_get;
    native.bind_source = &rsf_native_fg_bind_source;
    return rsf_test_d3d11_fg_api(&imports, &native);
}
