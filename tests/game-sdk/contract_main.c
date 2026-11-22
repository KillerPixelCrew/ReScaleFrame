// SPDX-License-Identifier: MIT
#include <rescaleframe/game_frame.h>
#include <stdio.h>

extern int rsf_test_plugin_contract(void);

static int check_frame_policy(void)
{
    const struct {
        rsf_screen_class screen;
        uint32_t flags;
        int sr;
        int fg;
    } cases[] = {
        {RSF_SCREEN_FLIGHT, 0, 1, 1},
        {RSF_SCREEN_VIDEO, 0, 0, 0},
        {RSF_SCREEN_LOADING, 0, 0, 0},
        {RSF_SCREEN_MENU, 0, 1, 0},
        {RSF_SCREEN_UNKNOWN, 0, 1, 0},
        {RSF_SCREEN_FLIGHT, RSF_FRAME_FLAG_NO_FG, 1, 0},
        {RSF_SCREEN_FLIGHT, RSF_FRAME_FLAG_NO_SR, 0, 0}
    };
    rsf_frame_record record = {0};
    record.struct_size = sizeof(record);
    record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    for (size_t i = 0; i < sizeof(cases) / sizeof(cases[0]); ++i) {
        record.screen = cases[i].screen;
        record.flags = cases[i].flags;
        if (rsf_frame_allows_sr(&record) != cases[i].sr ||
            rsf_frame_allows_fg(&record) != cases[i].fg) {
            fprintf(stderr, "Frame policy failed for screen %u flags %u.\n",
                    record.screen, record.flags);
            return 1;
        }
    }
    record.flags = 0;
    record.struct_size = 8;
    return rsf_frame_allows_sr(&record) || rsf_frame_allows_fg(&record);
}

int main(void)
{
    const int result = rsf_test_plugin_contract();
    return result ? result : check_frame_policy();
}
