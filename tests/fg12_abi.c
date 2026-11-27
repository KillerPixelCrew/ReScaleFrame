// SPDX-License-Identifier: MIT
#include <rescaleframe/native_fg_d3d12.h>
#include <stdio.h>

#define CHECK(condition) do { if (!(condition)) { printf("FG12 ABI failed at line %d\n", __LINE__); return 1; } } while (0)

static int reject_window(void* user, void* window)
{
    (void)user;
    (void)window;
    return 0;
}

int main(void)
{
    _Alignas(rsf_fg12_setup) uint32_t short_setup = sizeof(short_setup);
    CHECK(rsf_fg12_install(NULL) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_fg12_install_explicit((const rsf_fg12_setup*)&short_setup) ==
        RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    rsf_fg12_setup setup = {0};
    setup.struct_size = sizeof(setup);
    setup.runtime_directory_utf8 = "";
    setup.options.struct_size = sizeof(setup.options);
    setup.options.mode = RSF_FG_FIXED;
    setup.options.generated_frames = 1;
    setup.units_to_meters = 1;
    setup.accept_window = reject_window;
    CHECK(rsf_fg12_install(&setup) == RSF_BACKEND_ERROR_NOT_SUPPORTED);
    setup.options.reflex_mode = RSF_REFLEX_BOOST + 1;
    CHECK(rsf_fg12_install(&setup) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_fg12_latency(NULL) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    rsf_game_latency_event event = {0};
    event.struct_size = sizeof(event);
    event.marker = RSF_GAME_LATENCY_MODE;
    event.session_id = 1;
    CHECK(rsf_fg12_latency(&event) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(rsf_fg12_render_begin(0, 1) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_fg12_render_begin(1, 1) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(!rsf_fg12_end_session(0));
    rsf_fg12_cpu(NULL);
    rsf_fg12_capture(NULL, NULL, NULL, NULL);
    rsf_fg12_hudless(NULL, NULL);
    rsf_fg12_window(NULL);
    rsf_fg12_submitted(1, 0);
    CHECK(rsf_fg12_capture_outputs(NULL) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_fg12_capture_outputs("\xff") == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_fg12_capture_outputs("") == RSF_BACKEND_OK);

    _Alignas(rsf_native_fg_status) uint32_t short_status = sizeof(short_status);
    CHECK(!rsf_fg12_status_try((rsf_native_fg_status*)&short_status));
    CHECK(short_status == sizeof(short_status));
    rsf_native_fg_status status;
    status.struct_size = sizeof(status);
    CHECK(rsf_fg12_status(&status));
    CHECK(!status.available && !status.tagged_frames && status.requested.mode == RSF_FG_OFF);
    rsf_native_fg_options options = status.requested;
    options.mode = RSF_FG_FIXED;
    options.generated_frames = 3;
    rsf_fg12_options(&options);
    CHECK(rsf_fg12_status_try(&status));
    CHECK(status.requested.mode == RSF_FG_FIXED && status.requested.generated_frames == 3);
    CHECK(!status.vendor.active && !status.tagged_frames);
    return 0;
}
