/* SPDX-License-Identifier: MIT */
#include <rescaleframe/native_sr_vulkan.h>
#include <rescaleframe/native_fg_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include <rescaleframe/native_latency_vulkan.h>

typedef struct rsf_native_sr_fixture_calls {
    rsf_backend_result (*create)(const rsf_srvk_setup*, rsf_srvk**);
    rsf_backend_result (*plan)(const rsf_srvk*, uint32_t*, uint32_t*);
    rsf_backend_result (*evaluate)(rsf_srvk*, void*, const rsf_srvk_frame*);
    void (*retire)(rsf_srvk*, void*);
    rsf_backend_result (*destroy)(rsf_srvk*);
    rsf_backend_result (*preset)(rsf_srvk*, uint32_t);
    rsf_backend_result (*fallback)(const rsf_srvk_fallback*);
    rsf_backend_result (*fg_create)(const rsf_fgvk_setup*, rsf_fgvk**, uint64_t*);
    rsf_backend_result (*fg_destroy)(rsf_fgvk*);
    rsf_backend_result (*fg_drain)(rsf_fgvk*);
    rsf_backend_result (*fg_prepare)(rsf_fgvk*, void*, const rsf_fgvk_frame*);
    rsf_backend_result (*sl_initialize)(const rsf_slvk_setup*);
    rsf_vk_get_proc sl_lookup;
    rsf_backend_result (*sl_options)(uint32_t, uint32_t);
    void (*sl_cpu)(const rsf_game_cpu_event*);
    rsf_backend_result (*sl_shutdown)(void);
} rsf_native_sr_fixture_calls;

_Static_assert(sizeof(rsf_native_sr_fixture_calls) == 16 * sizeof(void*), "fixture call table ABI");

extern int rsf_native_sr_vulkan_fixture(const rsf_native_sr_fixture_calls*, int enabled, int argc, const char* argument);

int main(int argc, char** argv)
{
    static const rsf_native_sr_fixture_calls calls = {
        rsf_srvk_create, rsf_srvk_plan, rsf_srvk_evaluate, rsf_srvk_retire_commands,
        rsf_srvk_destroy, rsf_srvk_dlss_preset, rsf_srvk_spatial_fallback,
        rsf_fgvk_create, rsf_fgvk_destroy, rsf_fgvk_drain, rsf_fgvk_prepare,
        rsf_slvk_initialize, rsf_latencyvk_instance_proc, rsf_latencyvk_options,
        rsf_latencyvk_cpu, rsf_latencyvk_shutdown,
    };
#ifdef RSF_VULKAN_SR_ENABLED
    const int enabled = 1;
#else
    const int enabled = 0;
#endif
    return rsf_native_sr_vulkan_fixture(&calls, enabled, argc, argc > 1 ? argv[1] : 0);
}
