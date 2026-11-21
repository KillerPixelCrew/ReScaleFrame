// SPDX-License-Identifier: MIT
// The runtime setup is Rust (runtime_setup.rs, runtime_config.rs): the [UnitySR] keys and defaults,
// the start gates and result codes, the Mono wait and the rsf_fg12_setup and
// rsf_callback_host_setup descriptors. This file keeps the exported entry points and lends the
// typed calls into the Runtime the loader already loaded, the SDK header tables and the D3D12
// debug layer through rsf_unity_runtime_platform.
#include <rescaleframe/unity_runtime_host.h>
#include "runtime_platform.h"
#include <rescaleframe/fg_choice.h>
#include <rescaleframe/sha256_file.h>
#include <rescaleframe/dlss.h>
#include <windows.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <wrl/client.h>
#include <cstring>
#include <string>

extern "C" int rsf_unity_set_engine_spatial(uint32_t, uint32_t) noexcept;

// runtime_abi.rs mirrors these layouts and constants.
static_assert(sizeof(rsf_fg12_setup) == 152 && sizeof(rsf_native_fg_options) == 24);
static_assert(sizeof(rsf_callback_host_setup) == 128 && sizeof(rsf_game_probe) == 24);
static_assert(sizeof(rsf_unity_runtime_platform) == 104);
static_assert(RSF_DLSS_ENGINE_UNITY == 2 && RSF_BACKEND_OK == 0);

namespace {
constexpr rsf_unity_runtime_platform platform{
    RSF_UNITY_ENGINE_VERSION,
    RSF_UNITY_PROJECT_ID,
    [](uint32_t backend) noexcept -> const char* {
        const auto* info = rsf_fg_backend_info_for(backend);
        return info ? info->ini_key : nullptr;
    },
    [](const wchar_t* path, uint32_t fallback, uint32_t choices) noexcept {
        return rsf_fg_choice_start(path, fallback, choices);
    },
    []() noexcept { return rsf_fg_choice_get().choices; },
    [](const rsf_fg12_setup* setup) noexcept { return rsf_fg12_install(setup); },
    [](const char* directory) noexcept { return rsf_fg12_capture_outputs(directory); },
    []() noexcept {
        Microsoft::WRL::ComPtr<ID3D12Debug> debug;
        if (SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)))) debug->EnableDebugLayer();
    },
    [](uint32_t preset) noexcept -> uint32_t { return rsf_dlss_preset_valid(preset) ? 1u : 0u; },
    [](const wchar_t* path, char* digest) noexcept -> uint32_t {
        try {
            std::string hash;
            if (!rsf::sha256_file(path, hash) || hash.size() != 64) return 0;
            std::memcpy(digest, hash.c_str(), 65);
            return 1;
        } catch (...) { return 0; }
    },
    [](const rsf_callback_host_setup* setup) noexcept { return rsf_callback_host_start(setup); },
    []() noexcept { return rsf_callback_host_stop(); },
    rsf_unity_set_engine_spatial,
};
}

extern "C" uint32_t __stdcall rsf_unity_fg_start(const wchar_t* ini)
{
    return rsf_unity_mono_fg_start(ini, &platform);
}
extern "C" uint32_t __stdcall rsf_unity_sr_start(const wchar_t* ini)
{
    return rsf_unity_mono_sr_start(ini, &platform);
}
extern "C" uint32_t __stdcall rsf_unity_sr_stop(void*) { return rsf_unity_mono_sr_stop(&platform); }
extern "C" uint32_t __stdcall rsf_unity_start(const wchar_t* ini)
{
    return rsf_unity_mono_runtime_start(ini, &platform);
}
