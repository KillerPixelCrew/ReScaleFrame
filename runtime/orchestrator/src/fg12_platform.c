/* SPDX-License-Identifier: MIT */
/* Module lifetime of the Rust D3D12 FG capture owner. It reads capture
   preferences at module attach and releases capture tickets and resources at
   unload. */
#include "fg12_platform.h"
#include <stdlib.h>

static void fg12_capture_detach(void) { rsf_fg12_capture_detach(); }

#if defined(_MSC_VER)
static void __cdecl fg12_capture_attach(void)
{
    rsf_fg12_capture_attach();
    (void)atexit(fg12_capture_detach);
}

/* The CRT runs .CRT$XCU entries where it ran C++ static constructors. In a
   DLL, atexit registers on the module table, so detach runs at unload. */
#pragma section(".CRT$XCU", read)
__declspec(allocate(".CRT$XCU")) void(__cdecl* const rsf_fg12_capture_lifetime)(void) = fg12_capture_attach;
#pragma comment(linker, "/include:rsf_fg12_capture_lifetime")
#else
__attribute__((constructor)) static void fg12_capture_attach(void)
{
    rsf_fg12_capture_attach();
    (void)atexit(fg12_capture_detach);
}
#endif
