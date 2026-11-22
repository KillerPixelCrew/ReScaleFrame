// SPDX-License-Identifier: MIT
// Native declarations and guarded vendor calls for the Rust census.
#include <iterator>

#include "census_internal.h"
#include "ngx_platform.h"
#include "misc_platform.h"
#include "fsr_platform.h"
#include "wrapper_platform.h"

namespace rsf_census {

const FnDesc kFns[kFnCount] = {
#define X(id, api, name, mods) {#id, api, name, mods},
    RSF_CENSUS_FUNCTIONS(X)
#undef X
};

namespace {

int format_decimal(char* text, size_t size, double value) noexcept {
    return std::snprintf(text, size, "%.9g", value);
}

bool read_wide(const void* address, uint16_t* text, size_t capacity) noexcept {
    return read_arg_wstring(address, reinterpret_cast<wchar_t*>(text), capacity);
}

bool read_ngx_parameter(const void* parameters, size_t slot, const char* name,
                        void* output, int32_t* result) noexcept {
    using Getter = int32_t (*)(const void*, const char*, void*);
    auto get = reinterpret_cast<Getter>(vtable_slot(parameters, slot));
    return get && guard([&] { *result = get(parameters, name, output); });
}

bool read_streamline_frame(const void* token, uint32_t* value) noexcept {
    auto get = reinterpret_cast<uint32_t (*)(const void*)>(vtable_slot(token, 0));
    return get && guard([&] { *value = get(token); });
}

void write_streamline_output(void** output, void* value) noexcept {
    guard([&] { *output = value; });
}

void describe_resource12(rsf_census_json* state, const char* key, const void* resource) noexcept {
    describe_d3d_resource(state, key, resource, true);
}

constexpr uint16_t kVkEntries[] = {
#define RSF_CENSUS_VK_MAP(name) static_cast<uint16_t>(Fn::name),
    RSF_CENSUS_VK_ENTRIES(RSF_CENSUS_VK_MAP)
#undef RSF_CENSUS_VK_MAP
};
static_assert(std::size(kVkEntries) == static_cast<size_t>(RSF_CENSUS_VK_COUNT));
constinit const rsf_census_vk_platform kVk = {safe_read, read_arg_string};

constexpr uint16_t kNgxEntries[] = {
#define RSF_CENSUS_NGX_ID(name, api, symbol, modules) static_cast<uint16_t>(Fn::name),
    RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, D3D12, nullptr, nullptr)
    RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, D3D11, nullptr, nullptr)
    RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, VULKAN, nullptr, nullptr)
#undef RSF_CENSUS_NGX_ID
};
static_assert(std::size(kNgxEntries) == static_cast<size_t>(RSF_CENSUS_NGX_ENTRIES));
constinit const rsf_census_ngx_platform kNgx = {
    safe_read,
    read_arg_string,
    read_wide,
    read_ngx_parameter,
    describe_d3d_resource,
    format_decimal,
};

constexpr uint16_t kMiscEntries[] = {
#define RSF_CENSUS_MISC_ID(name) static_cast<uint16_t>(Fn::name),
    RSF_CENSUS_MISC_ENTRIES(RSF_CENSUS_MISC_ID)
#undef RSF_CENSUS_MISC_ID
};
static_assert(std::size(kMiscEntries) == static_cast<size_t>(RSF_MISC_COUNT));
constinit const rsf_census_misc_platform kMisc = {
    safe_read, read_arg_string,
    read_wide,
    read_streamline_frame,
    write_streamline_output,
    describe_resource12,
    format_decimal,
};

constexpr uint16_t kFsrEntries[] = {
#define RSF_CENSUS_FSR_ID(name) static_cast<uint16_t>(Fn::name),
    RSF_CENSUS_FSR_ENTRIES(RSF_CENSUS_FSR_ID)
#undef RSF_CENSUS_FSR_ID
};
static_assert(std::size(kFsrEntries) == static_cast<size_t>(RSF_FSR_COUNT));
constinit const rsf_census_fsr_platform kFsr = {
    safe_read,
    vtable_slot,
    describe_resource12,
    format_decimal,
};

constinit const rsf_census_vendor_platform kVendors = {
    kVkEntries, &kVk, kNgxEntries, &kNgx, kFsrEntries, &kFsr, kMiscEntries, &kMisc,
};

} // namespace
} // namespace rsf_census

extern "C" rsf_census_result rsf_census_install(const rsf_census_options* options) {
    using namespace rsf_census;
    static constinit const rsf_census_core_platform platform = {
        kFns, static_cast<uint16_t>(kFnCount), static_cast<uint16_t>(Fn::CensusEvent),
        &kHookPlatform, &kVendors,
    };
    return rsf_census_install_impl(options, &platform);
}
