/* SPDX-License-Identifier: GPL-3.0-only */
#include "preferences.h"
#include <windows.h>
#include <stdio.h>

void rsf_preferences_read(const wchar_t* path, uint32_t* enabled, uint32_t* quality)
{
    if (!path || !path[0] || !enabled || !quality) {
        return;
    }
    const UINT saved_enabled = GetPrivateProfileIntW(L"Rendering", L"Enabled", 2, path);
    const UINT saved_quality = GetPrivateProfileIntW(L"Rendering", L"Quality", 5, path);
    if (saved_enabled <= 1) {
        *enabled = saved_enabled;
    }
    if (saved_quality <= 4) {
        *quality = saved_quality;
    }
}

int rsf_preferences_write(const wchar_t* path, uint32_t enabled, uint32_t quality)
{
    wchar_t level[2] = {(wchar_t)(L'0' + quality), L'\0'};
    if (!path || !path[0] || enabled > 1 || quality > 4) {
        return 0;
    }
    return WritePrivateProfileStringW(L"Rendering", L"Quality", level, path) &&
           WritePrivateProfileStringW(L"Rendering", L"Enabled", enabled ? L"1" : L"0", path);
}
