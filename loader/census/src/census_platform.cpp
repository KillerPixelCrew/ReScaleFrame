// SPDX-License-Identifier: MIT
// Native wrapper catalog, call records, and the private Rust session entry.
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

void put_decimal(rsf_census_json* json, const char* key, double value) noexcept {
    char text[40];
    std::snprintf(text, sizeof(text), "%.9g", value);
    rsf_census_json_raw(json, key, text);
}

void register_wrappers_vk(void** table) noexcept {
    static constexpr uint16_t entries[] = {
#define RSF_CENSUS_VK_MAP(name) static_cast<uint16_t>(Fn::name),
        RSF_CENSUS_VK_ENTRIES(RSF_CENSUS_VK_MAP)
#undef RSF_CENSUS_VK_MAP
    };
    static_assert(std::size(entries) == static_cast<size_t>(RSF_CENSUS_VK_COUNT));
    static const rsf_census_vk_platform platform = {safe_read, read_arg_string};
    void* wrappers[RSF_CENSUS_VK_COUNT]{};
    rsf_census_vk_bind(&platform, entries, wrappers);
    for (size_t i = 0; i < std::size(entries); ++i) table[entries[i]] = wrappers[i];
}

void register_wrappers_ngx(void** table) noexcept {
    static constexpr uint16_t entries[] = {
#define RSF_CENSUS_NGX_ID(name, api, symbol, modules) static_cast<uint16_t>(Fn::name),
        RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, D3D12, nullptr, nullptr)
        RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, D3D11, nullptr, nullptr)
        RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, VULKAN, nullptr, nullptr)
#undef RSF_CENSUS_NGX_ID
    };
    static_assert(std::size(entries) == static_cast<size_t>(RSF_CENSUS_NGX_ENTRIES));
    static const rsf_census_ngx_platform platform = {
        safe_read,
        read_arg_string,
        [](const void* address, uint16_t* text, size_t capacity) {
            return read_arg_wstring(address, reinterpret_cast<wchar_t*>(text), capacity);
        },
        [](const void* parameters, size_t slot, const char* name, void* output, int32_t* result) {
            using Getter = int32_t (*)(const void*, const char*, void*);
            auto get = reinterpret_cast<Getter>(vtable_slot(parameters, slot));
            return get && guard([&] { *result = get(parameters, name, output); });
        },
        [](rsf_census_json* state, const char* key, const void* resource, bool d3d12) {
            describe_d3d_resource(state, key, resource, d3d12);
        },
        put_decimal,
    };
    void* wrappers[RSF_CENSUS_NGX_ENTRIES]{};
    rsf_census_ngx_bind(&platform, entries, wrappers);
    for (size_t i = 0; i < std::size(entries); ++i) table[entries[i]] = wrappers[i];
}

void register_wrappers_misc(void** table) noexcept {
    static constexpr uint16_t entries[] = {
#define RSF_CENSUS_MISC_ID(name) static_cast<uint16_t>(Fn::name),
        RSF_CENSUS_MISC_ENTRIES(RSF_CENSUS_MISC_ID)
#undef RSF_CENSUS_MISC_ID
    };
    static_assert(std::size(entries) == static_cast<size_t>(RSF_MISC_COUNT));
    static const rsf_census_misc_platform platform = {
        safe_read, read_arg_string,
        [](const void* p, uint16_t* text, size_t capacity) {
            return read_arg_wstring(p, reinterpret_cast<wchar_t*>(text), capacity);
        },
        [](const void* token, uint32_t* value) {
            auto f = reinterpret_cast<uint32_t (*)(const void*)>(vtable_slot(token, 0));
            return f && guard([&] { *value = f(token); });
        },
        [](void** output, void* value) { guard([&] { *output = value; }); },
        [](rsf_census_json* state, const char* key, const void* p) {
            describe_d3d_resource(state, key, p, true);
        },
        put_decimal,
    };
    void* wrappers[RSF_MISC_COUNT]{};
    rsf_census_misc_bind(&platform, entries, wrappers);
    for (size_t i = 0; i < std::size(entries); ++i) table[entries[i]] = wrappers[i];
}

void register_wrappers_fsr(void** table) noexcept {
    static constexpr uint16_t entries[] = {
#define RSF_CENSUS_FSR_ID(name) static_cast<uint16_t>(Fn::name),
        RSF_CENSUS_FSR_ENTRIES(RSF_CENSUS_FSR_ID)
#undef RSF_CENSUS_FSR_ID
    };
    static_assert(std::size(entries) == static_cast<size_t>(RSF_FSR_COUNT));
    static const rsf_census_fsr_platform platform = {
        safe_read,
        vtable_slot,
        [](rsf_census_json* state, const char* key, const void* object) {
            describe_d3d_resource(state, key, object, true);
        },
        put_decimal,
    };
    void* wrappers[RSF_FSR_COUNT]{};
    rsf_census_fsr_bind(&platform, entries, wrappers);
    for (size_t i = 0; i < std::size(entries); ++i) table[entries[i]] = wrappers[i];
}

} // namespace rsf_census

extern "C" rsf_census_result rsf_census_install(const rsf_census_options* options) {
    using namespace rsf_census;
    static const rsf_census_core_platform platform = {
        kFns, static_cast<uint16_t>(kFnCount), static_cast<uint16_t>(Fn::CensusEvent), hooks_install,
    };
    return rsf_census_install_impl(options, &platform);
}
