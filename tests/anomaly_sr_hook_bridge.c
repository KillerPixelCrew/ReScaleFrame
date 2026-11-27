/* SPDX-License-Identifier: MIT */
/* Launcher for the Rust SR hook fixture (sr_stage/hook_fixture.rs), which calls the production C11 leaves linked in
   here. The rmNear case owns a real MinHook detour on a target in this executable called through the engine's Windows ABI. */
#include "native_geometry.h"
#include "native_geometry_platform.h"
#include "native_hooks.h"
#include "sr_stage_platform.h"
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _MSC_VER
#define RSF_NOINLINE __declspec(noinline)
#else
#define RSF_NOINLINE __attribute__((noinline))
#endif

static int near_calls, callback_calls;
static void* callback_self;

static RSF_NOINLINE void __fastcall near_fixture(void* self)
{
    ++near_calls;
    if (!self) abort();
}

static void callback(void* self)
{
    ++callback_calls;
    callback_self = self;
}

typedef struct near_calls_table {
    void (*invoke)(void*);
    void (*set_callback)(bool);
    bool (*disable)(void*);
    void (*counts)(int*, int*, void**);
} near_calls_table;
_Static_assert(sizeof(near_calls_table) == 32, "rmNear call table");

static void invoke(void* self) { near_fixture(self); }
static void set_callback(bool on) { rsf_anomaly_geometry_set_near(on ? callback : NULL); }
static bool disable(void* owner) { return rsf_anomaly_hooks_disable((rsf_anomaly_hook_set*)owner); }
static void counts(int* near_count, int* callbacks, void** seen)
{
    *near_count = near_calls;
    *callbacks = callback_calls;
    *seen = callback_self;
}

int32_t rsf_anomaly_sr_hook_near(const near_calls_table*, void*);
int32_t rsf_test_anomaly_sr_hooks(int, const char* const*, int (*)(size_t, size_t));

/* Redirects the selected build's geometry rmNear site onto near_fixture. */
static int exercise_near_slot(size_t build, size_t rm_near)
{
    const rsf_anomaly_hook_site* sites = rsf_anomaly_geometry_hook_sites(build);
    unsigned char* base = (unsigned char*)GetModuleHandleW(NULL);
    const IMAGE_DOS_HEADER* dos = (const IMAGE_DOS_HEADER*)base;
    const IMAGE_NT_HEADERS64* pe = (const IMAGE_NT_HEADERS64*)(base + dos->e_lfanew);
    if (!sites) {
        puts("FAIL reading the geometry hook sites");
        return 1;
    }
    rsf_anomaly_hook_site redirect = sites[rm_near];
    redirect.rva = (uint32_t)((unsigned char*)(void*)&near_fixture - base);
    memcpy(redirect.expected, (const void*)&near_fixture, sizeof(redirect.expected));
    rsf_anomaly_hook_set hooks = {0};
    if (!rsf_anomaly_hooks_install(&hooks, base, pe->OptionalHeader.SizeOfImage, &redirect, 1)) {
        puts("FAIL installing the geometry rmNear fixture detour");
        return 1;
    }
    const near_calls_table calls = {invoke, set_callback, disable, counts};
    const int32_t failures = rsf_anomaly_sr_hook_near(&calls, &hooks);
    rsf_anomaly_geometry_set_near(NULL);
    return failures;
}

int main(int argc, char** argv)
{
    return rsf_test_anomaly_sr_hooks(argc, (const char* const*)argv, exercise_near_slot);
}
