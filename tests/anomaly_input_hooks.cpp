// SPDX-License-Identifier: MIT
#include "build_profile.h"
#include "engine_slots.h"
#include <string_view>

extern "C" int rsf_anomaly_input_fixture(size_t build, uint32_t, uint32_t,
    decltype(&rsf::anomaly::guarded_copy), rsf_anomaly_slot_exchange, const wchar_t*) noexcept;

int main(int argc, char** argv) {
    using namespace rsf::anomaly;
    const bool avx = argc > 1 && std::string_view(argv[1]) == "avx";
    const auto& build = avx ? avx_build : standard_build;
    const wchar_t* path = argc <= 2 ? nullptr : avx ?
        L"D:/Anomaly 1.5.3/bin/AnomalyDX11AVX.exe" : L"D:/Anomaly 1.5.3/bin/AnomalyDX11.exe";
    return rsf_anomaly_input_fixture(avx ? 1u : 0u, build.image_size(), build.native->device_frame_rva,
        guarded_copy, compare_slot, path);
}
