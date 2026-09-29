// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/ac7_scene_color.h>
#include "../loader/proxy/src/preferences.h"
#include <windows.h>
#include <dxgiformat.h>
#include <cmath>
#include <cstdio>
#include <initializer_list>

int main()
{
    bool passed = true;
    auto check = [&](bool good, const char* why) {
        if (!good) { std::fprintf(stderr, "%s\n", why); passed = false; }
    };
    // Follow repeated quality changes rather than deriving the next target from the previous one.
    for (const unsigned percent : {100u, 50u, 67u, 58u, 34u, 100u, 50u}) {
        const float scale = rsf_ac7_translucency_scale(percent, 50);
        check(std::fabs(3840.0f * float(percent) / 100.0f * scale - 1920.0f) < 0.001f,
              "The briefing must retain its vanilla half-output width at every scene quality.");
        check(std::fabs(2160.0f * float(percent) / 100.0f * scale - 1080.0f) < 0.001f,
              "The briefing must retain its vanilla half-output height at every scene quality.");
        const float native_scale = rsf_ac7_translucency_scale(percent, 100);
        check(std::fabs(3840.0f * float(percent) / 100.0f * native_scale - 3840.0f) < 0.001f &&
              std::fabs(2160.0f * float(percent) / 100.0f * native_scale - 2160.0f) < 0.001f,
              "The full-resolution briefing must remain at output size across scene presets.");
    }
    check(rsf_ac7_translucency_scale(0, 50) == 0.5f, "Unknown scale defaults to a native scene.");
    check(rsf_ac7_translucency_scale(50, 0) == 1.0f, "The diagnostic scene-relative mode remains explicit.");
    rsf_frame_tap_target_draw draw{};
    draw.render_target = &draw;
    draw.depth_bound = 1;
    draw.target_count = 1;
    draw.target_samples = 1;
    draw.target_format = DXGI_FORMAT_R16G16B16A16_FLOAT;
    check(rsf_ac7_separate_translucency_draw(&draw) != 0, "The layer's first draw is recognized without cached identity.");
    draw.depth_bound = 0;
    check(!rsf_ac7_separate_translucency_draw(&draw), "Fullscreen float postprocessing is not layer geometry.");
    draw.depth_bound = 1;
    draw.target_count = 3;
    check(!rsf_ac7_separate_translucency_draw(&draw), "The MRT base pass is not separate translucency.");

    wchar_t directory[MAX_PATH]{}, path[MAX_PATH]{};
    check(GetTempPathW(MAX_PATH, directory) != 0 && GetTempFileNameW(directory, L"rsf", 0, path),
          "A temporary preferences file must be created.");
    uint32_t enabled = 1, quality = 3;
    rsf_preferences_read(path, &enabled, &quality);
    check(enabled == 1 && quality == 3, "First launch preserves enabled/Performance defaults.");
    for (uint32_t preset = 0; preset <= 4; ++preset) {
        check(rsf_preferences_write(path, preset % 2, preset) != 0, "Preferences must be writable.");
        enabled = 9; quality = 9;
        rsf_preferences_read(path, &enabled, &quality);
        check(enabled == preset % 2 && quality == preset, "Every preset and enable choice must survive reopening.");
    }
    WritePrivateProfileStringW(L"Rendering", L"Quality", L"99", path);
    quality = 3;
    rsf_preferences_read(path, &enabled, &quality);
    check(quality == 3, "An invalid saved preset must keep the caller's valid fallback.");
    check(!rsf_preferences_write(path, 1, 5), "An invalid preset must not be persisted.");
    DeleteFileW(path);
    return passed ? 0 : 1;
}
