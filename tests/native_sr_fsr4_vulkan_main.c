/* SPDX-License-Identifier: MIT */
#include "native_sr_fsr4_vulkan_fixture.h"
#include <stdio.h>
#include <wchar.h>

int32_t rsf_fixture_fsr4sr_public_abi(
    int32_t (*create)(const rsf_fsr4_bridge_create*, rsf_fsr4_bridge**));
int32_t rsf_fixture_fsr4sr_device(const rsf_fsr4sr_fixture_calls*, const wchar_t*, size_t);

int wmain(int argc, wchar_t** argv) {
    const int32_t contract = rsf_fixture_fsr4sr_public_abi(RsfFsr4BridgeCreate);
    if (contract) return contract;
    if (argc == 1) {
        puts("{\"contract_checks\":\"passed\",\"device_test\":false}");
        return 0;
    }
    if (argc != 3 || wcscmp(argv[1], L"--device")) return 4;
    static const rsf_fsr4sr_fixture_calls calls = {
        rsf_srvk_fsr4_create, rsf_srvk_fsr4_plan, rsf_fsr4vk_info, rsf_srvk_fsr4_evaluate,
        rsf_srvk_fsr4_retire_commands, rsf_srvk_fsr4_submitted, rsf_srvk_fsr4_destroy,
    };
    return rsf_fixture_fsr4sr_device(&calls, argv[2], wcslen(argv[2]));
}
