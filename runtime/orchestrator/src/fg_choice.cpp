// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/fg_choice.h>
#include <windows.h>
#include <mutex>
#include <string>
namespace {
std::mutex guard;
std::wstring file;
rsf_fg_choice choice{};
bool allowed(uint32_t backend, uint32_t choices) {
    return (backend == 0 || rsf_fg_backend_known(backend)) && (choices & (1u << backend)) != 0;
}
}
extern "C" RSF_RUNTIME_API uint32_t rsf_fg_choice_start(const wchar_t* path, uint32_t fallback, uint32_t choices) try {
    std::lock_guard<std::mutex> lock(guard);
    file = path ? path : L"";
    choices |= 1u;
    const auto saved = file.empty() ? fallback : GetPrivateProfileIntW(L"Rendering", L"FrameGeneration", fallback, file.c_str());
    choice = {allowed(saved, choices) ? saved : allowed(fallback, choices) ? fallback : 0u, choices, 0};
    return choice.backend;
}
catch (...) { return fallback; }
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fg_choice_save(uint32_t backend) {
    std::lock_guard<std::mutex> lock(guard);
    if (!allowed(backend, choice.choices)) return choice.last_result = RSF_BACKEND_ERROR_NOT_SUPPORTED;
    if (file.empty()) return choice.last_result = RSF_BACKEND_ERROR_FEATURE_FAILED;
    const wchar_t value[]{wchar_t(L'0' + backend), L'\0'};
    if (!WritePrivateProfileStringW(L"Rendering", L"FrameGeneration", value, file.c_str()))
        return choice.last_result = RSF_BACKEND_ERROR_FEATURE_FAILED;
    choice.backend = backend; choice.last_result = RSF_BACKEND_OK; return RSF_BACKEND_OK;
}
extern "C" RSF_RUNTIME_API rsf_fg_choice rsf_fg_choice_get() {
    std::lock_guard<std::mutex> lock(guard); return choice;
}
