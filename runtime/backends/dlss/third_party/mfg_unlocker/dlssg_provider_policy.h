#pragma once

#include <Windows.h>

#include <cstdint>

extern "C" {
uint32_t rsf_mfg_provider_profile(const wchar_t* path);
uint32_t rsf_mfg_supported_provider(HMODULE module, const wchar_t* path);
uint32_t rsf_mfg_retained_provider(HMODULE module);
}

namespace dlssg_provider_policy
{
enum class Profile : uint32_t
{
    Unsupported = 0,
    Early = 1,
    LegacySlot11 = 2,
    LegacySlot10 = 3,
    LegacySlot9 = 4,
    V3109 = 5,
    V310916 = 6,
};

inline bool IsSupportedProvider(HMODULE module, const wchar_t* path) noexcept
{
    return rsf_mfg_supported_provider(module, path) != 0;
}

inline bool IsSupportedRetainedProvider(HMODULE module) noexcept
{
    return rsf_mfg_retained_provider(module) != 0;
}
}
