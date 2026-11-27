/* SPDX-License-Identifier: MIT */
/* Launcher for the Rust hook-set fixture (native_hooks/fixture.rs): two real x64 targets in this executable. The
   hook set, MinHook calls and profile records are Rust-owned; the guarded copy comes from the native leaves. */
#ifdef _MSC_VER
#define RSF_NOINLINE __declspec(noinline)
#else
#define RSF_NOINLINE __attribute__((noinline))
#endif

typedef int (*function_fn)(int);
static RSF_NOINLINE int target(int value) { return value + 7; }
static RSF_NOINLINE int untouched(int value) { return value * 5 + 11; }

int rsf_test_anomaly_native_hooks(function_fn, function_fn);

int main(void) { return rsf_test_anomaly_native_hooks(target, untouched); }
