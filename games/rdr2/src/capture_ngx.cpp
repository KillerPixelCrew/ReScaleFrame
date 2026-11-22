// SPDX-License-Identifier: MIT
// Only NGX C++ virtual dispatch stays here. Snapshot policy and registration live in Rust.
#include <d3d12.h>
#include <rescaleframe/census_d3d12_observer.h>
#include <cstdint>

namespace {
template <class T>
bool get(const void* parameters, uint32_t slot, const char* key, void* output)
{
    using Read = int32_t (*)(const void*, const char*, T*);
    const auto table = *static_cast<void* const* const*>(parameters);
    const auto read = reinterpret_cast<Read>(table[slot]);
    return read && read(parameters, key, static_cast<T*>(output)) == 1;
}
}

static bool read_ngx(
    const void* parameters, uint32_t slot, const char* key, void* output) noexcept try
{
    if (!parameters) return false;
    // MSVC's overloaded Get slot order is audited in loader/census/src/wrap_ngx.cpp.
    switch (slot) {
    case 8: return get<void*>(parameters, slot, key, output);
    case 9: return get<ID3D12Resource*>(parameters, slot, key, output);
    case 11: return get<int32_t>(parameters, slot, key, output);
    case 12: return get<uint32_t>(parameters, slot, key, output);
    case 14: return get<float>(parameters, slot, key, output);
    default: return false;
    }
}
catch (...) { return false; }

extern "C" bool rsf_rdr2_capture_start(const wchar_t*, bool,
    decltype(&rsf_census_observe_d3d12), decltype(&rsf_census_observe_ngx_d3d12), decltype(&read_ngx));

#ifdef RSF_RDR2_CAPTURE_FIXTURE
extern "C" bool rsf_rdr2_install_capture_fixture(const wchar_t* directory)
{
    return rsf_rdr2_capture_start(directory, true, rsf_census_observe_d3d12, rsf_census_observe_ngx_d3d12, read_ngx);
}
#else
extern "C" bool rsf_rdr2_install_native_capture(const wchar_t* directory)
{
    return rsf_rdr2_capture_start(directory, false, rsf_census_observe_d3d12, rsf_census_observe_ngx_d3d12, read_ngx);
}
#endif
