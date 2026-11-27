/* SPDX-License-Identifier: MIT */
#include <rescaleframe/d3d11_present_api.h>

#ifdef RSF_RUNTIME_BUILD
#error This contract must import the real Runtime DLL.
#endif

/* A conditional between function pointers requires compatible signatures.
   sizeof checks the types without reading through the null table pointer. */
#define RSF_CHECK_SLOT(name) \
    _Static_assert(sizeof(1 ? &rsf_d3d11_present_##name : ((rsf_d3d11_present_api*)0)->name) > 0, \
        "presentation API slot " #name);
RSF_CHECK_SLOT(install)
RSF_CHECK_SLOT(prepare)
RSF_CHECK_SLOT(create_for_hwnd)
RSF_CHECK_SLOT(host)
RSF_CHECK_SLOT(graphics)
RSF_CHECK_SLOT(queue)
RSF_CHECK_SLOT(command_list)
RSF_CHECK_SLOT(retire)
RSF_CHECK_SLOT(is_owner)
RSF_CHECK_SLOT(has_owner)
RSF_CHECK_SLOT(provider)
RSF_CHECK_SLOT(session)
RSF_CHECK_SLOT(backend)
RSF_CHECK_SLOT(backend_choices)
RSF_CHECK_SLOT(request)
RSF_CHECK_SLOT(switch_result)
RSF_CHECK_SLOT(generation)
RSF_CHECK_SLOT(begin)
RSF_CHECK_SLOT(marker)
RSF_CHECK_SLOT(acquire)
RSF_CHECK_SLOT(input)
RSF_CHECK_SLOT(abort)
#undef RSF_CHECK_SLOT

int rsf_d3d11_present_api_fixture(rsf_get_d3d11_present_api_fn);

int main(void) {
    return rsf_d3d11_present_api_fixture(&rsf_get_d3d11_present_api);
}
