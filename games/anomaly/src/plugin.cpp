// SPDX-License-Identifier: MIT
#include <rescaleframe/game_api.h>
#include "native_observer.h"
#include "build_profile.h"

extern "C" {
rsf_detection rsf_anomaly_detect(const rsf_game_probe*, const rsf_anomaly_build_identity*, size_t) noexcept;
rsf_result rsf_anomaly_publish_api(uint32_t, rsf_game_plugin_api*, rsf_detect_game_fn,
    const rsf_game_hooks_api*) noexcept;
}

namespace {
rsf_detection detect(const rsf_game_probe* probe) noexcept {
    const auto& builds = rsf::anomaly::build_identities;
    return rsf_anomaly_detect(probe, builds.data(), builds.size());
}
constexpr rsf_game_hooks_api hooks{
    rsf::anomaly::prepare, rsf::anomaly::start, rsf::anomaly::quiesce,
    rsf::anomaly::stop, rsf::anomaly::status,
};
}
extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept {
    return rsf_anomaly_publish_api(requested_abi, api, detect, &hooks);
}
