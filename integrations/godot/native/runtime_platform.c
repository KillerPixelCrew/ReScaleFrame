/* SPDX-License-Identifier: MIT */
/* The native side of ReScaleFrame.Godot.Native.dll that Rust cannot express: the CMake project
   version and the DLL unload hook. The GDExtension classes, every rsfg_* export and all decisions
   are Rust (native/lib.rs, and the Runtime's Vulkan host behind exports.def). */
#include <rescaleframe/native_cpu.h>
#include "api.h"

#include <rescaleframe/fg_choice.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/version.h>
#include <windows.h>
#include <stddef.h>
#include <stdlib.h>

const char* rsf_godot_version(void)
{
    return RSF_VERSION_STRING;
}

void rsf_godot_host_unload(void);

/* Rust statics need an explicit release at the DLL's CRT boundary. The hook registers with atexit
   from a CRT initializer, which is what the former C++ static destructor did, so the CRT runs it
   at DLL detach and at process exit. */
static void host_unload_at_exit(void)
{
    rsf_godot_host_unload();
}

#if defined(_MSC_VER)
static void __cdecl register_host_unload(void)
{
    atexit(host_unload_at_exit);
}

#pragma section(".CRT$XCU", read)
__declspec(allocate(".CRT$XCU")) static void(__cdecl* const register_host_unload_hook)(void) =
    register_host_unload;
#else
__attribute__((constructor)) static void register_host_unload(void)
{
    atexit(host_unload_at_exit);
}
#endif
