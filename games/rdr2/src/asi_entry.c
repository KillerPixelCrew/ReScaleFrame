/* SPDX-License-Identifier: MIT */
#include "asi_platform.h"
#include <rescaleframe/sha256_file.h>
#include <windows.h>

/* Capture services. */
bool rsf_rdr2_install_native_capture(const wchar_t* directory);

_Static_assert(RSF_CAPTURE_OK == 0 && RSF_CENSUS_OK == 0, "diagnostic success codes");
_Static_assert(RSF_CENSUS_ERROR_INVALID_ARGUMENT == -1, "census invalid argument code");

static const rsf_rdr2_asi_services services = {
    .fingerprint = rsf_common_sha256_file,
    .measure = rsf_measure_module_code,
    .dump = rsf_dump_when_decrypted,
    .modules = rsf_write_module_list,
    .census_install = rsf_census_install,
    .census_flush = rsf_census_flush,
    .census_counters = rsf_census_get_counters,
#ifdef RSF_HAVE_FRAME_CAPTURE
    .frame_initialise = rsf_capture_initialise,
    .frame_trigger = rsf_capture_trigger,
#endif
    .native_capture = rsf_rdr2_install_native_capture,
    .social_current_index = rsf_rdr2_social_club_current_index,
};

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved)
{
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        rsf_rdr2_asi_process_attach(instance, &services);
    }
    return TRUE;
}
