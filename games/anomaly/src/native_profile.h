// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <span>

namespace rsf::anomaly {
// Anomaly 1.5.3 ships the DX11 renderer as two executables built from the same source: the standard build and the AVX
// build (same code, AVX codegen). Each has its own exact profile below; build_profile.h selects one by file name and
// SHA-256 at startup. RVAs, expected bytes and frame layouts of one build never apply to the other.
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
// Where the 16 expected bytes of a site are not unique in .text: the shortest prefix at the same RVA that is. It starts
// with the site's 16 bytes and is checked before any hook is written (tests/anomaly_*_evidence.py prove uniqueness and
// minimality per build).
struct UniqueGuard {
    const char* name;
    uint32_t rva;
    const char* expected_hex;
};
// True when `hex` (lowercase, even length) is at image + rva inside image_size. The image is the mapped executable.
inline bool unique_guard_matches(const std::byte* image, uint32_t image_size, const UniqueGuard& guard) noexcept {
    const auto length = std::strlen(guard.expected_hex) / 2;
    if (!image || guard.rva > image_size || length > image_size - guard.rva) return false;
    const auto digit = [](char c) { return c <= '9' ? c - '0' : c - 'a' + 10; };
    for (size_t i = 0; i < length; ++i)
        if (image[guard.rva + i] != static_cast<std::byte>(digit(guard.expected_hex[2 * i]) * 16 + digit(guard.expected_hex[2 * i + 1])))
            return false;
    return true;
}
// Every guard in `guards` whose RVA is `rva` matches (none listed is also true).
inline bool unique_guards_match(const std::byte* image, uint32_t image_size, std::span<const UniqueGuard> guards,
    uint32_t rva) noexcept {
    for (const auto& guard : guards)
        if (guard.rva == rva && !unique_guard_matches(image, image_size, guard)) return false;
    return true;
}

// Native observer boundaries, matched against RTTI virtual slots and decompiled bodies (docs/research/anomaly-native-hooks.md,
// docs/research/anomaly-avx-profile.md). Order is the observer's detour order.
inline constexpr std::size_t native_site_count = 8;
struct NativeProfile {
    uint32_t image_size;           // IMAGE_OPTIONAL_HEADER64::SizeOfImage
    uint32_t device_width_rva;     // Device.dwWidth (u32), dwHeight follows
    uint32_t device_height_rva;
    uint32_t device_frame_rva;     // Device.dwFrame
    std::array<NativeSite, native_site_count> native_sites;
    std::span<const UniqueGuard> native_unique_guards;
};

// ---- Standard 1.5.3 DX11 (AnomalyDX11.exe) -------------------------------------------------------------------------
inline constexpr std::array<UniqueGuard, 2> native_standard_unique{{
    {"CRender::Render", 0x8977c0, "488bc44889480853565741544155415641574881ec20"},
    {"CRender::reset_begin", 0x83a020, "48894c2408565741564883ec3048c7442420feffffff48895c2460488bf9"},
}};
inline constexpr NativeProfile native_standard{
    .image_size = 0xfca000,
    .device_width_rva = 0xd37668,
    .device_height_rva = 0xd3766c,
    .device_frame_rva = 0xd37680,
    .native_sites = {{
        {"IRenderDeviceRender::Begin", 0x822cb0, signature("40534883ec20488bd9488d0d000f5400")},
        {"IRenderDeviceRender::End", 0x822eb0, signature("4883ec28833d498a4c00007406488b01")},
        {"IRenderDeviceRender::SetCacheXform", 0x823250, signature("40534883ec20488d0dbb095400498bd8")},
        {"CRender::Render", 0x8977c0, signature("488bc448894808535657415441554156")},
        {"CHUDManager::RenderUI", 0x0f1e60, signature("40534883ec40f7051823bf0000100000")},
        {"CRender::RenderToTarget", 0x898640, signature("40534883ec30448bc24183e801744d41")},
        {"CRender::reset_begin", 0x83a020, signature("48894c2408565741564883ec3048c744")},
        {"CRender::reset_end", 0x83a1c0, signature("40574883ec5048895c2468488bf94889")},
    }},
    .native_unique_guards = native_standard_unique,
};

// ---- AVX 1.5.3 DX11 (AnomalyDX11AVX.exe) ---------------------------------------------------------------------------
// Same functions, transferred by RTTI slot and call-graph position and re-read (docs/research/anomaly-avx-profile.md).
// .data/.bss lie 0x12000 lower than in the standard build; each global is still proven from its own references.
inline constexpr std::array<UniqueGuard, 2> native_avx_unique{{
    {"CRender::Render", 0x88d420, "488bc44889480853565741544155415641574881ec50"},
    {"CRender::reset_begin", 0x8312d0, "48894c2408565741564883ec3048c7442420feffffff48895c2460488bf9"},
}};
inline constexpr NativeProfile native_avx{
    .image_size = 0xfb8000,
    .device_width_rva = 0xd25668,
    .device_height_rva = 0xd2566c,
    .device_frame_rva = 0xd25680,
    .native_sites = {{
        {"IRenderDeviceRender::Begin", 0x81a930, signature("40534883ec20488bd9488d0d80725300")},
        {"IRenderDeviceRender::End", 0x81ab40, signature("4883ec28833db9ed4b00007406488b01")},
        {"IRenderDeviceRender::SetCacheXform", 0x81aee0, signature("40534883ec20488d0d2b6d5300498bd8")},
        {"CRender::Render", 0x88d420, signature("488bc448894808535657415441554156")},
        {"CHUDManager::RenderUI", 0x0f0810, signature("40534883ec40f7056819be0000100000")},
        {"CRender::RenderToTarget", 0x88e290, signature("40534883ec30448bc24183e801744d41")},
        {"CRender::reset_begin", 0x8312d0, signature("48894c2408565741564883ec3048c744")},
        {"CRender::reset_end", 0x831470, signature("40574883ec5048895c2468488bf94889")},
    }},
    .native_unique_guards = native_avx_unique,
};
static_assert(native_standard.device_height_rva == native_standard.device_width_rva + 4, "Device.dwHeight follows dwWidth");
static_assert(native_avx.device_height_rva == native_avx.device_width_rva + 4, "Device.dwHeight follows dwWidth");
}
