// SPDX-License-Identifier: MIT
#include <rescaleframe/plugin_session.h>

typedef struct session_functions {
    rsf_result (*prepare)(const rsf_plugin_session_options*, rsf_plugin_session**);
    rsf_result (*start)(rsf_plugin_session*);
    rsf_result (*quiesce)(rsf_plugin_session*);
    rsf_result (*status)(rsf_plugin_session*, rsf_game_renderer_status*);
    rsf_result (*stop)(rsf_plugin_session*);
} session_functions;

extern int rsf_test_plugin_session(const session_functions*);

int main(void)
{
    const session_functions functions = {
        rsf_plugin_session_prepare, rsf_plugin_session_start, rsf_plugin_session_quiesce,
        rsf_plugin_session_status, rsf_plugin_session_stop
    };
    return rsf_test_plugin_session(&functions);
}
