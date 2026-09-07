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

/* The eligibility rules are inline in the header, so they are compiled here as C as well as being
   exercised as C++ in the contract test. A rule that only builds in one language is a rule a plugin
   cannot use, and plugins are the reason this SDK is MIT and header-only. */
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
