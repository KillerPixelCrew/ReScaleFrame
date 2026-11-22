/* SPDX-License-Identifier: MIT */
/* Only NGX virtual dispatch and its foreign exception boundary stay native. Snapshot policy and
   registration live in Rust. */
#include <rescaleframe/foreign_guard.h>
#include <rescaleframe/census_d3d12_observer.h>
#include <stdbool.h>
#include <stdint.h>
#include <windows.h>

typedef int32_t (*ngx_get_fn)(const void* parameters, const char* key, void* output);
typedef bool (*read_ngx_fn)(const void* parameters, uint32_t slot, const char* key, void* output);

/* MSVC's overloaded Get slot order follows tests/vendor_census_ngx_params.h. Every overload
   takes a pointer to the value, so one call shape serves all of them. */
static bool slot_supported(uint32_t slot)
{
    return slot == 8 || slot == 9 || slot == 11 || slot == 12 || slot == 14;
}

static bool dispatch(const void* parameters, uint32_t slot, const char* key, void* output)
{
    const void* const* table = *(const void* const* const*)parameters;
    const ngx_get_fn read = (ngx_get_fn)table[slot];
    return read && read(parameters, key, output) == 1;
}

/* The NGX implementation is foreign C++ on a borrowed parameter block: every fault and C++
   exception reads as unavailable. */
static bool read_ngx(const void* parameters, uint32_t slot, const char* key, void* output)
{
    if (!parameters || !slot_supported(slot)) return false;
    bool read = false;
    RSF_FOREIGN_TRY { read = dispatch(parameters, slot, key, output); }
    RSF_FAULT_CATCH { read = false; }
    return read;
}

typedef void (*observe_d3d12_fn)(void (*)(void*), void (*)(void*, uint32_t, void* const*),
    void (*)(void*, uint32_t, void* const*), void (*)(void*), void (*)(void*), void (*)(void*, uint32_t));
typedef void (*observe_ngx_fn)(void (*)(void*, const void*, uint32_t),
    void (*)(void*, void*, const void*, uint32_t, uint64_t), void (*)(void*));

bool rsf_rdr2_capture_start(const wchar_t*, bool, observe_d3d12_fn, observe_ngx_fn, read_ngx_fn);

#ifdef RSF_RDR2_CAPTURE_FIXTURE
bool rsf_rdr2_install_capture_fixture(const wchar_t* directory)
{
    return rsf_rdr2_capture_start(directory, true, rsf_census_observe_d3d12, rsf_census_observe_ngx_d3d12, read_ngx);
}
#else
bool rsf_rdr2_install_native_capture(const wchar_t* directory)
{
    return rsf_rdr2_capture_start(directory, false, rsf_census_observe_d3d12, rsf_census_observe_ngx_d3d12, read_ngx);
}
#endif