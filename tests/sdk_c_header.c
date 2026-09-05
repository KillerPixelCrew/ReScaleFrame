#include <rescaleframe/game_api.h>

/* Compile the public header as C, independently of the C++ implementation. */
_Static_assert(sizeof(rsf_result) == 4, "Result width must remain explicit.");
_Static_assert(sizeof(rsf_detection) == 4, "Detection width must remain explicit.");

int rsf_sdk_c_header_check(void)
{
    rsf_game_plugin_api api = {0};
    api.struct_size = sizeof(api);
    return api.detect == 0;
}
