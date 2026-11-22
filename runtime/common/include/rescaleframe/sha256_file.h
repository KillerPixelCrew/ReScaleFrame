/* SPDX-License-Identifier: MIT */
#pragma once
#include <windows.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif
/* Writes 32 digest bytes on success. Sharing is a CreateFileW sharing mask. */
uint32_t rsf_common_sha256_file(const wchar_t* path, uint32_t sharing, unsigned char* digest);
#ifdef __cplusplus
}
#include <string>

namespace rsf {
inline bool sha256_file(const wchar_t* path, unsigned char (&bytes)[32],
                        DWORD share = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
{
    return rsf_common_sha256_file(path, share, bytes) != 0;
}
/* Lower-case hex form. */
inline bool sha256_file(const wchar_t* path, std::string& digest)
{
    unsigned char bytes[32]{};
    if (!sha256_file(path, bytes)) {
        return false;
    }
    constexpr char hex[] = "0123456789abcdef";
    digest.clear();
    digest.reserve(64);
    for (const unsigned char value : bytes) {
        digest += hex[value >> 4];
        digest += hex[value & 15];
    }
    return true;
}
}
#endif
