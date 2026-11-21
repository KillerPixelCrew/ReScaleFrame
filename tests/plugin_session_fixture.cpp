// SPDX-License-Identifier: MIT
#include <rescaleframe/game_api.h>

namespace {
const rsf_game_host_services* host;
unsigned quiesce_calls;
unsigned stop_calls;

rsf_result prepare(const rsf_game_prepare_args* args) {
    host = args->host;
    if (host->session_id == 2) throw 1;
    return RSF_OK;
}
rsf_result start(const rsf_game_start_args*) {
    host->log(host->user, "start");
    return RSF_OK;
}
rsf_result quiesce(const rsf_game_control_args*) {
    ++quiesce_calls;
    return (host->session_id == 2 || host->session_id == 3) && quiesce_calls == 1 ? RSF_ERROR_BUSY : RSF_OK;
}
rsf_result stop(const rsf_game_control_args*) {
    ++stop_calls;
    return host->session_id == 2 && stop_calls == 1 ? RSF_ERROR_BUSY : RSF_OK;
}
rsf_result status(rsf_game_renderer_status*) {
    host->log(host->user, "status");
    return RSF_OK;
}
}

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(uint32_t abi, rsf_game_plugin_api* api) {
    if (abi != RSF_GAME_ABI_VERSION || api->struct_size < sizeof(*api)) return RSF_ERROR_ABI_MISMATCH;
    api->abi_version = abi;
    api->detect = [](const rsf_game_probe*) { return RSF_GAME_RECOGNIZED; };
    api->hooks = {prepare, start, quiesce, stop, status};
    return RSF_OK;
}
