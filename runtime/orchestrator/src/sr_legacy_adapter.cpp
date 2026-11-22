// SPDX-License-Identifier: MIT
#include "sr_legacy_adapter.h"
#include <rescaleframe/input_dump.h>
#include <rescaleframe/sr_bridge.h>

namespace {
struct LegacyServices {
    decltype(&rsf_sr_bridge_create) create;
    decltype(&rsf_sr_bridge_select) select;
    decltype(&rsf_sr_bridge_get_status) status;
    decltype(&rsf_sr_bridge_set_auto_exposure) exposure;
    decltype(&rsf_sr_bridge_evaluate) evaluate;
    decltype(&rsf_sr_bridge_warm_up) warm_up;
    decltype(&rsf_sr_bridge_destroy) destroy;
    decltype(&rsf_motion_resolve_create) create_resolve;
    decltype(&rsf_motion_resolve_run) resolve;
    decltype(&rsf_motion_resolve_depth) depth;
    decltype(&rsf_motion_resolve_motion) motion;
    decltype(&rsf_motion_resolve_destroy) destroy_resolve;
    decltype(&rsf_input_dump_active) dump_active;
    decltype(&rsf_input_dump_sr) dump_frame;
};
static_assert(sizeof(LegacyServices) == 14 * sizeof(void*));

constexpr LegacyServices services{
    rsf_sr_bridge_create, rsf_sr_bridge_select, rsf_sr_bridge_get_status, rsf_sr_bridge_set_auto_exposure,
    rsf_sr_bridge_evaluate, rsf_sr_bridge_warm_up, rsf_sr_bridge_destroy,
    rsf_motion_resolve_create, rsf_motion_resolve_run, rsf_motion_resolve_depth, rsf_motion_resolve_motion, rsf_motion_resolve_destroy,
    rsf_input_dump_active, rsf_input_dump_sr,
};
}

extern "C" rsf_backend_result rsf_core_sr_legacy_create(const rsf_sr_session_setup*, float units_to_meters,
    uint32_t standard_depth, uint32_t auto_exposure, const LegacyServices*, rsf_sr_legacy_adapter**);

rsf_backend_result rsf_sr_legacy_create(void* device, uint32_t width, uint32_t height,
    const char* fsr2, const char* fsr3, const char* fsr4, const char* xess,
    float units_to_meters, uint32_t standard_depth, uint32_t auto_exposure, rsf_backend_log_fn log, void* user,
    rsf_sr_legacy_adapter** out)
{
    rsf_sr_session_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_SR_SESSION_ABI_VERSION;
    setup.open.struct_size = sizeof(setup.open);
    setup.open.abi_version = RSF_BACKEND_ABI_VERSION;
    setup.open.api = RSF_API_D3D11;
    setup.open.device = device;
    setup.open.output_width = width;
    setup.open.output_height = height;
    // The legacy pipeline setup does not carry the projection far-plane convention.
    setup.open.depth_infinite = standard_depth == 0;
    setup.open.log = log;
    setup.open.log_user = user;
    setup.fsr2_directory_utf8 = fsr2;
    setup.fsr3_directory_utf8 = fsr3;
    setup.fsr4_directory_utf8 = fsr4;
    setup.xess_directory_utf8 = xess;
    return rsf_core_sr_legacy_create(&setup, units_to_meters, standard_depth, auto_exposure, &services, out);
}
