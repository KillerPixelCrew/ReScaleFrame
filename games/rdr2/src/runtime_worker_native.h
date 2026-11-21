// SPDX-License-Identifier: MIT
#pragma once
#include "runtime_setup.h"
#include <rescaleframe/sha256_file.h>
#include <cstring>
#include <string>

inline bool rsf_rdr2_fingerprint(const wchar_t* executable, char* digest) noexcept try
{
    std::string hash;
    if (!rsf::sha256_file(executable, hash) || hash.size() != 64) return false;
    std::memcpy(digest, hash.c_str(), 65);
    return true;
}
catch (...) { return false; }
