/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
/* Lower-case hex SHA-256 of a file, with every handle owned by a guard so no exit path leaks one.
   Header-only; consumers link bcrypt (rsf_runtime_common does). The unity-mono plugin and the
   dinput8 loader proxy are separate modules that do not link the runtime; the plugin is C++ and can
   include this header directly if it adds this include directory, the proxy is C and keeps its own
   copy (or a small C twin of this loop). Reads the file with full sharing so a running game's
   binary can be hashed. */
#include <windows.h>
#include <bcrypt.h>
#include <string>

namespace rsf {
/* Raw 32-byte digest. `share` is the CreateFile sharing mode: the default lets a running game's
   binary be hashed, fsr4_compat's FILE_SHARE_READ is also valid. */
inline bool sha256_file(const wchar_t* path, unsigned char (&bytes)[32],
                        DWORD share = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
{
    struct File {
        HANDLE handle;
        ~File() { if (handle != INVALID_HANDLE_VALUE) CloseHandle(handle); }
    } file{CreateFileW(path, GENERIC_READ, share,
                       nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr)};
    if (file.handle == INVALID_HANDLE_VALUE) {
        return false;
    }
    struct Provider {
        BCRYPT_ALG_HANDLE algorithm = nullptr;
        BCRYPT_HASH_HANDLE hash = nullptr;
        ~Provider()
        {
            if (hash) BCryptDestroyHash(hash);
            if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0);
        }
    } provider;
    if (BCryptOpenAlgorithmProvider(&provider.algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0 ||
        BCryptCreateHash(provider.algorithm, &provider.hash, nullptr, 0, nullptr, 0, 0) < 0) {
        return false;
    }
    unsigned char buffer[65536];
    for (;;) {
        DWORD read = 0;
        if (!ReadFile(file.handle, buffer, sizeof(buffer), &read, nullptr)) {
            return false;
        }
        if (!read) {
            break;
        }
        if (BCryptHashData(provider.hash, buffer, read, 0) < 0) {
            return false;
        }
    }
    return BCryptFinishHash(provider.hash, bytes, sizeof(bytes), 0) >= 0;
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
