// SPDX-License-Identifier: MIT
#include "contact_shadow.h"
#include "shader_bytes.h"
#include <windows.h>
#include <wincrypt.h>

namespace {
bool sha256(const uint8_t* bytes, uint32_t size, uint8_t* digest) noexcept
{
    HMODULE module = LoadLibraryExW(L"crypt32.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) return false;
    auto hash = reinterpret_cast<decltype(&CryptHashCertificate2)>(
        reinterpret_cast<void*>(GetProcAddress(module, "CryptHashCertificate2")));
    DWORD count = 32;
    const bool ok = hash && hash(L"SHA256", 0, nullptr, bytes, size, digest, &count) && count == 32;
    FreeLibrary(module);
    return ok;
}

static_assert(static_cast<int32_t>(rsf_ac7_contact_shadow_result::unmatched) == 0);
static_assert(static_cast<int32_t>(rsf_ac7_contact_shadow_result::patched) == 1);
static_assert(static_cast<int32_t>(rsf_ac7_contact_shadow_result::refused) == 2);
}

extern "C" int32_t rsf_core_ac7_contact_shadow_correct(const uint8_t*, size_t,
    bool (*sha256)(const uint8_t*, uint32_t, uint8_t*), uint8_t* (*resize)(void*, size_t), void* user) noexcept;
extern "C" uint32_t rsf_core_ac7_contact_shadow_crc(const uint8_t*, size_t) noexcept;

rsf_ac7_contact_shadow_result rsf_ac7_contact_shadow_correct(
    const void* bytes, size_t size, std::vector<uint8_t>& output) noexcept
{
    output.clear();
    const auto result = static_cast<rsf_ac7_contact_shadow_result>(
        rsf_core_ac7_contact_shadow_correct(static_cast<const uint8_t*>(bytes), size, sha256,
            rsf::ac7::resize_shader_bytes, &output));
    if (result != rsf_ac7_contact_shadow_result::patched) output.clear();
    return result;
}

uint32_t rsf_ac7_contact_shadow_crc(const void* bytes, size_t size) noexcept
{
    return rsf_core_ac7_contact_shadow_crc(static_cast<const uint8_t*>(bytes), size);
}
