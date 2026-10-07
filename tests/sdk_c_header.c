/**
 * @file
 * Compile the public Game SDK and inline frame policy as C.
 * This OBJECT target has no executable entry point. The corresponding C++ runtime
 * eligibility checks live in plugin_contract.cpp; both languages must accept the
 * self-contained public headers.
 */
#include <rescaleframe/game_api.h>
#include <rescaleframe/game_frame.h>

/* Compile the public headers as C, independently of the C++ implementation. */
_Static_assert(sizeof(rsf_result) == 4, "Result width must remain explicit.");
_Static_assert(sizeof(rsf_detection) == 4, "Detection width must remain explicit.");
_Static_assert(sizeof(rsf_frame_id) == 8, "A frame identifier must not wrap in a long session.");

int rsf_sdk_c_header_check(void)
{
    rsf_game_plugin_api api = {0};
    api.struct_size = sizeof(api);
    return api.detect == 0;
}

/* Compile inline eligibility helpers as C; plugin_contract exercises them at runtime in C++. */
int rsf_sdk_c_frame_check(void)
{
    rsf_frame_record record = {0};
    record.struct_size = sizeof(record);
    record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.screen = RSF_SCREEN_FLIGHT;
    if (!rsf_frame_allows_sr(&record) || !rsf_frame_allows_fg(&record)) {
        return 0;
    }
    record.screen = RSF_SCREEN_VIDEO;
    if (rsf_frame_allows_sr(&record) || rsf_frame_allows_fg(&record)) {
        return 0;
    }
    return 1;
}
