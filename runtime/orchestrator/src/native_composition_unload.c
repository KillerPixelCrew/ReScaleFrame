/* SPDX-License-Identifier: MIT */
#include <rescaleframe/native_composition.h>
#include <rescaleframe/native_translucency.h>
#include <stdlib.h>

void rsf_core_native_exposure_release(void);
void rsf_native_regions_release(void);
void rsf_native_reinsert_release(void);

/* Covers DLL and embedded carrier unload if explicit graphics teardown was skipped. The release
   hook registers with atexit from a CRT initializer, which is what a C++ static destructor does,
   so the CRT runs it at process exit and at DLL detach. */
static void release_at_unload(void)
{
    rsf_native_composition_release();
    rsf_native_reinsert_release();
    rsf_core_native_exposure_release();
    rsf_native_regions_release();
    rsf_native_translucency_release();
}

#if defined(_MSC_VER)
static void __cdecl register_release_at_unload(void)
{
    atexit(release_at_unload);
}

#pragma section(".CRT$XCU", read)
__declspec(allocate(".CRT$XCU")) static void(__cdecl* const register_release_hook)(void) =
    register_release_at_unload;
#else
__attribute__((constructor)) static void register_release_at_unload(void)
{
    atexit(release_at_unload);
}
#endif
