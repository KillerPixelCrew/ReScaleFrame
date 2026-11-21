// SPDX-License-Identifier: MIT
#pragma once
#include "shader_loader.h"
#include "shader_loader_platform.h"
#include <rescaleframe/shader_patch.h>

namespace rsf::anomaly::motion {
extern "C" {
void rsf_anomaly_runtime_pack_close(void*) noexcept;
const rsf_anomaly_shader_admitted* rsf_anomaly_runtime_pack_manifest(const void*) noexcept;
const rsf_shader_patch_api* rsf_anomaly_runtime_pack_api(const void*) noexcept;
int32_t rsf_anomaly_runtime_pack_prepare(void*, const rsf_anomaly_shader_request*, rsf_patch_output*) noexcept;
}

inline const rsf_shader_patch_api& runtime_pack_api(const void* pack) noexcept {
    return *rsf_anomaly_runtime_pack_api(pack);
}

void* open_runtime_pack(const wchar_t* pack, const wchar_t* cache, std::byte* image, const MotionProfile&,
    std::string& error);
Prepared prepare_runtime_pack(void* pack, const rsf_anomaly_shader_request&, const ManifestEntry&, Reflector&);
}
