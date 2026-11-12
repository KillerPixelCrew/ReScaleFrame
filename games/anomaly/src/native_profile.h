// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <array>
#include <cstddef>
#include <cstdint>

namespace rsf::anomaly {
inline constexpr char dx11_hash[] = "c857b6f49d32c62245611b26106862a773185533856e8b858a7d7e849ed2fb4e";
inline constexpr char avx_hash[] = "2032acfc041e215508692ef4079ef504c7bbabf1793d5668084099f734b92f12";
struct NativeSite {
    const char* name;
    uint32_t rva;
    std::array<std::byte, 16> expected;
};
consteval std::array<std::byte, 16> signature(const char* hex) {
    std::array<std::byte, 16> result{};
    for (size_t i = 0; i < result.size(); ++i) {
        const auto digit = [](char c) consteval { return c <= '9' ? c - '0' : c - 'a' + 10; };
        result[i] = static_cast<std::byte>(digit(hex[i * 2]) * 16 + digit(hex[i * 2 + 1]));
    }
    return result;
}
// Standard 1.5.3 DX11, matched against RTTI virtual slots and decompiled bodies.
// RVAs are not used on the AVX executable. See docs/research/anomaly-native-hooks.md.
inline constexpr uint32_t native_image_size = 0xfca000;
inline constexpr uint32_t device_width_rva = 0xd37668;
inline constexpr uint32_t device_height_rva = 0xd3766c;
inline constexpr uint32_t device_frame_rva = 0xd37680;
inline constexpr std::array<NativeSite, 8> native_sites{{
    {"IRenderDeviceRender::Begin", 0x822cb0, signature("40534883ec20488bd9488d0d000f5400")},
    {"IRenderDeviceRender::End", 0x822eb0, signature("4883ec28833d498a4c00007406488b01")},
    {"IRenderDeviceRender::SetCacheXform", 0x823250, signature("40534883ec20488d0dbb095400498bd8")},
    {"CRender::Render", 0x8977c0, signature("488bc448894808535657415441554156")},
    {"CHUDManager::RenderUI", 0x0f1e60, signature("40534883ec40f7051823bf0000100000")},
    {"CRender::RenderToTarget", 0x898640, signature("40534883ec30448bc24183e801744d41")},
    {"CRender::reset_begin", 0x83a020, signature("48894c2408565741564883ec3048c744")},
    {"CRender::reset_end", 0x83a1c0, signature("40574883ec5048895c2468488bf94889")}
}};
}
