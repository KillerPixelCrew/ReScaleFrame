/* SPDX-License-Identifier: GPL-3.0-only */
/* Compile and run as C11, with a dllimport reference to the actual Runtime DLL. */
#include <rescaleframe/overlay_api.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>

#ifdef RSF_RUNTIME_BUILD
#error The C11 client must import Runtime, never embed its owner.
#endif

_Static_assert(offsetof(rsf_overlay_api, struct_size) == 0, "size prefix");
_Static_assert(offsetof(rsf_overlay_api, abi_version) == sizeof(uint32_t), "version prefix");
_Static_assert(offsetof(rsf_overlay_api, configure) == 8, "first function offset");
_Static_assert(offsetof(rsf_overlay_service_setup, abi_version) == sizeof(uint32_t), "setup version prefix");
_Static_assert(offsetof(rsf_overlay_capture, struct_size) == 0, "capture size prefix");

int main(void) {
    rsf_overlay_api api;
    rsf_get_overlay_api_fn query = &rsf_get_overlay_api;
    rsf_overlay_capture capture;
    rsf_render_settings settings;
    rsf_overlay_service_setup setup;
    memset(&api, 0, sizeof(api));
    if (query(NULL) != RSF_ERROR_INVALID_ARGUMENT) return 1;
    api.struct_size = (uint32_t)sizeof(api) - 1u; api.abi_version = RSF_OVERLAY_API_VERSION;
    if (query(&api) != RSF_ERROR_INVALID_ARGUMENT || api.configure) return 1;
    api.struct_size = sizeof(api); api.abi_version = RSF_OVERLAY_API_VERSION + 1u;
    if (query(&api) != RSF_ERROR_ABI_MISMATCH || api.configure) return 1;
    api.abi_version = RSF_OVERLAY_API_VERSION;
    if (query(&api) != RSF_OK || api.struct_size != sizeof(api) || api.abi_version != RSF_OVERLAY_API_VERSION) return 1;
    if (!api.configure || !api.present || !api.notify || !api.visible || !api.set_visible || !api.toggle ||
        !api.capture || !api.status || !api.settings_get || !api.settings_apply) return 1;

    memset(&capture, 0, sizeof(capture)); capture.struct_size = sizeof(capture);
    if (api.capture(&capture) != RSF_OK || capture.started || capture.captured) return 1;
    memset(&settings, 0, sizeof(settings));
    settings.struct_size = sizeof(settings); settings.abi_version = RSF_RENDER_FRONTEND_ABI_VERSION;
    if (api.settings_get(&settings) != RSF_ERROR_NOT_READY) return 1;
    if (api.visible() != 0u || api.set_visible(1u) != RSF_ERROR_NOT_READY) return 1;
    memset(&setup, 0, sizeof(setup));
    setup.struct_size = sizeof(setup); setup.abi_version = RSF_OVERLAY_API_VERSION;
    if (api.configure(&setup) != RSF_ERROR_INVALID_ARGUMENT) return 1;
    setup.abi_version = RSF_OVERLAY_API_VERSION + 1u;
    if (api.configure(&setup) != RSF_ERROR_ABI_MISMATCH) return 1;
    puts("PASS: C11 overlay API header and real Runtime DLL-import query");
    return 0;
}
