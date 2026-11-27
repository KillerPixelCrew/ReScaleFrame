/* SPDX-License-Identifier: MIT */
#include "capture/capture_fixture_abi.h"

extern int RSF_FIXTURE_ENTRY(const char*, const rsf_fixture_vk_capture_api*);

int main(int argc, char** argv)
{
    /* Automatic storage: the dllimport addresses are not link-time constants. */
    const rsf_fixture_vk_capture_api api = {
        rsf_input_dump_vk_create, rsf_input_dump_vk_snapshot,
        rsf_input_dump_vk_drain, rsf_input_dump_vk_destroy,
    };
    return argc == 2 ? RSF_FIXTURE_ENTRY(argv[1], &api) : 2;
}
