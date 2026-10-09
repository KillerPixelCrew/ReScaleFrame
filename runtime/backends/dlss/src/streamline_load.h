// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/backend.h>
#include <rescaleframe/log.h>
#include "../../common/sr_helpers.h"
#include <string>

// Defined next to the Streamline headers in dlss_streamline.cpp, the one place sl_security.h may be
// included. Answers false in a build that cannot verify signatures.
bool rsf_dlss_verify_runtime_signature(const wchar_t* path);

namespace rsf {
// UTF-8 to wide, false for null, empty or invalid text.
inline bool widen_utf8(const char* utf8, std::wstring& out)
{
    out.clear();
    if (!utf8 || !*utf8) return false;
    const int needed = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, utf8, -1, nullptr, 0);
    if (needed <= 0) return false;
    out.resize(static_cast<size_t>(needed) - 1);
    MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, utf8, -1, out.data(), needed);
    return true;
}
// Loads sl.interposer.dll from an absolute path, after the signature check when one is required.
// This puts a signed NVIDIA module into a game process, so the path alone must never decide which
// module answers: the search is the same fixed one for every Streamline owner.
inline HMODULE load_interposer(const wchar_t* path, bool require_signature, rsf_backend_log_fn log, void* user)
{
    // LoadLibraryExW with a search flag refuses a relative path, so name that here instead of a bare error.
    if (!path || !((path[0] && path[1] == L':' && (path[2] == L'\\' || path[2] == L'/')) ||
                   (path[0] == L'\\' && path[1] == L'\\'))) {
        say(log, user, "interposer path is not absolute: %ls", path ? path : L"(null)");
        return nullptr;
    }
    if (require_signature && !rsf_dlss_verify_runtime_signature(path)) {
        say(log, user, "interposer at %ls could not be verified as signed", path);
        return nullptr;
    }
    return load_module(path, log, user);
}
}
