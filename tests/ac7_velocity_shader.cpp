// SPDX-License-Identifier: MIT
#include <filesystem>

extern "C" int rsf_ac7_velocity_warp_fixture(const wchar_t* directory);

int main(int argc, char** argv)
{
    const auto retained = argc == 2 ? std::filesystem::path(argv[1]) : std::filesystem::path{};
    return rsf_ac7_velocity_warp_fixture(retained.c_str());
}
