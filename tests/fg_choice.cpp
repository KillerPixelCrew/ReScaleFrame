// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/fg_choice.h>
#include <windows.h>
#include <string>
#include <cstdio>
int main() {
    wchar_t root[MAX_PATH]{}, path[MAX_PATH]{};
    if (!GetTempPathW(MAX_PATH, root) || !GetTempFileNameW(root, L"rsf", 0, path)) return 1;
    bool ok = rsf_fg_choice_start(path, RSF_FG_BACKEND_DLSS, 0x3b) == RSF_FG_BACKEND_DLSS;
    ok &= rsf_fg_choice_save(RSF_FG_BACKEND_FSR3) == RSF_BACKEND_OK;
    auto choice = rsf_fg_choice_get();
    ok &= choice.backend == RSF_FG_BACKEND_FSR3 && choice.last_result == RSF_BACKEND_OK;
    ok &= rsf_fg_choice_start(path, RSF_FG_BACKEND_DLSS, 0x3b) == RSF_FG_BACKEND_FSR3;
    ok &= rsf_fg_choice_save(0) == RSF_BACKEND_OK;
    ok &= rsf_fg_choice_start(path, RSF_FG_BACKEND_DLSS, 0x3b) == 0;
    ok &= rsf_fg_choice_save(2) == RSF_BACKEND_ERROR_NOT_SUPPORTED;
    ok &= rsf_fg_choice_get().backend == 0;
    ok &= rsf_fg_choice_start(path, 0, 0x39) == 0;
    ok &= rsf_fg_choice_save(RSF_FG_BACKEND_DLSS) == RSF_BACKEND_ERROR_NOT_SUPPORTED;
    ok &= rsf_fg_choice_save(RSF_FG_BACKEND_XESS) == RSF_BACKEND_OK;
    ok &= rsf_fg_choice_start(path, 0, 0x39) == RSF_FG_BACKEND_XESS;
    ok &= WritePrivateProfileStringW(L"Rendering", L"FrameGeneration", L"2", path) != 0;
    ok &= rsf_fg_choice_start(path, RSF_FG_BACKEND_FSR3, 0x39) == RSF_FG_BACKEND_FSR3;
    rsf_fg_choice_start(nullptr, RSF_FG_BACKEND_DLSS, 0x3b);
    ok &= rsf_fg_choice_save(RSF_FG_BACKEND_FSR3) == RSF_BACKEND_ERROR_FEATURE_FAILED;
    choice = rsf_fg_choice_get();
    ok &= choice.backend == RSF_FG_BACKEND_DLSS && choice.last_result == RSF_BACKEND_ERROR_FEATURE_FAILED;
    DeleteFileW(path);
    std::puts(ok ? "PASS: saved/reloaded provider, Off, renderer mask, corrupt value and failed save" : "FAIL: generation provider persistence");
    return ok ? 0 : 1;
}
