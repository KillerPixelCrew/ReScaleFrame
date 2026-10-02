// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/ac7_motion_capture.h>
#include <cstdio>
#include <cstring>
#include <limits>

int main()
{
    auto check = [](const rsf_ac7_velocity_facts& f, const char* expected) {
        const char* actual = rsf_ac7_velocity_reason(&f);
        if (std::strcmp(actual, expected) == 0) return true;
        std::fprintf(stderr, "expected %s, got %s\n", expected, actual); return false;
    };
    rsf_ac7_velocity_facts f{};
    f.fields_valid = 1; f.visible = 1; f.mobility = 2; f.relevance = 0x4001;
    f.radius = 0.5f; f.distance_squared = 10000; f.lod_factor = 1; f.minimum_size = 0.5f;
    if (!check(f, "size")) return 1;
    f.minimum_size = 50; f.radius = 100; // Exactly representable equality.
    if (!check(f, "size")) return 1;
    f.minimum_size = 0.5f; f.radius = 2; f.has_velocity_called = 1; f.history_checked = 1;
    if (!check(f, "missing_history")) return 1;
    f.history_found = 1;
    if (!check(f, "unchanged_transform")) return 1;
    f.camera_cut = 1;
    if (!check(f, "camera_cut")) return 1;
    f.accepted = 1;
    if (!check(f, "accepted")) return 1;
    f.accepted = 0; f.has_velocity_called = 0; f.radius = std::numeric_limits<float>::quiet_NaN();
    if (!check(f, "nonfinite")) return 1;
    f.fields_valid = 0;
    if (!check(f, "unreadable")) return 1;
    f.fields_valid = 1; f.visible = 0;
    if (!check(f, "visibility")) return 1;
    f.visible = 1; f.mobility = 0;
    if (!check(f, "mobility")) return 1;
    f.mobility = 2; f.relevance = 1;
    return check(f, "material_or_main_pass") ? 0 : 1;
}
