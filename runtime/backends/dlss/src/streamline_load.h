// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/backend.h>
#include <rescaleframe/log.h>
#include <windows.h>

// Defined next to the Streamline headers in dlss_streamline.cpp, the one place sl_security.h may be
// included. Answers false in a build that cannot verify signatures.
bool rsf_dlss_verify_runtime_signature(const wchar_t* path);

extern "C" uint32_t rsf_streamline_absolute_path(const wchar_t* path);
extern "C" HMODULE rsf_core_load_absolute_library(const wchar_t* path, uint32_t* error);

static_assert(sizeof(wchar_t) == sizeof(uint16_t));

namespace rsf {
// Loads sl.interposer.dll from an absolute path, after the signature check when one is required.
// This puts a signed NVIDIA module into a game process, so the path alone must never decide which
// module answers: the search is the same fixed one for every Streamline owner.
inline HMODULE load_interposer(const wchar_t* path, bool require_signature, rsf_backend_log_fn log, void* user)
{
    // LoadLibraryExW with a search flag refuses a relative path, so name that here instead of a bare error.
    if (!rsf_streamline_absolute_path(path)) {
        rsf_logf(log, user, "interposer path is not absolute: %ls", path ? path : L"(null)");
        return nullptr;
    }
    if (require_signature && !rsf_dlss_verify_runtime_signature(path)) {
        rsf_logf(log, user, "interposer at %ls could not be verified as signed", path);
        return nullptr;
    }
    uint32_t error = 0;
    const HMODULE module = rsf_core_load_absolute_library(path, &error);
    if (!module) rsf_logf(log, user, "could not load %ls (Windows error %lu)", path, static_cast<unsigned long>(error));
    return module;
}
}
