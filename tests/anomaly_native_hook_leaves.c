/* SPDX-License-Identifier: MIT */
/* Launcher for the Rust hook-set fixture (native_hooks/fixture.rs): the production MinHook platform, the guarded copy
   and two real x64 targets in this executable. The profile records are Rust-owned. */
#include "native_hooks.h"
#include "native_memory.h"

#ifdef _MSC_VER
#define RSF_NOINLINE __declspec(noinline)
#else
#define RSF_NOINLINE __attribute__((noinline))
#endif

typedef int (*function_fn)(int);
static RSF_NOINLINE int target(int value) { return value + 7; }
static RSF_NOINLINE int untouched(int value) { return value * 5 + 11; }

static bool copy(void* output, const void* input, size_t size) { return guarded_copy(output, input, size); }

int rsf_test_anomaly_native_hooks(const rsf_anomaly_hook_platform*, bool (*)(void*, const void*, size_t),
    function_fn, function_fn);

int main(void) { return rsf_test_anomaly_native_hooks(&rsf_anomaly_native_hook_platform, copy, target, untouched); }