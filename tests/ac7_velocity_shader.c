/* SPDX-License-Identifier: MIT */
#include <wchar.h>

int rsf_ac7_velocity_warp_fixture(const wchar_t* directory);

int wmain(int argc, wchar_t** argv)
{
    return rsf_ac7_velocity_warp_fixture(argc == 2 ? argv[1] : L"");
}
