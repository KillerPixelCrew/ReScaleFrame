// SPDX-License-Identifier: MIT
// Native wrapper catalog, call records, and the private Rust session entry.
#include <cmath>
#include <iterator>
#include <new>

#include "census_internal.h"
#include "ngx_platform.h"
#include "wrapper_platform.h"

namespace rsf_census {

const FnDesc kFns[kFnCount] = {
#define X(id, api, name, mods) {#id, api, name, mods},
    RSF_CENSUS_FUNCTIONS(X)
#undef X
};

void register_wrappers_vk(void** table) noexcept {
    static constexpr Fn entries[] = {
#define RSF_CENSUS_VK_MAP(name) Fn::name,
        RSF_CENSUS_VK_ENTRIES(RSF_CENSUS_VK_MAP)
#undef RSF_CENSUS_VK_MAP
    };
    static_assert(std::size(entries) == RSF_CENSUS_VK_COUNT);
    static_assert(sizeof(rsf_census_call_state) <= sizeof(rsf_census_call_storage));
    static_assert(alignof(rsf_census_call_state) <= alignof(rsf_census_call_storage));
    static const rsf_census_wrapper_services services = {
        [](uint16_t entry) { return orig_by(entries[entry]); },
        [](rsf_census_call_storage* storage, uint16_t entry, void* caller) -> rsf_census_json* {
            auto* call = ::new (static_cast<void*>(storage)) rsf_census_call_state{};
            rsf_census_call_begin(call, static_cast<uint16_t>(entries[entry]), caller);
            return call->full ? &call->json : nullptr;
        },
        [](rsf_census_call_storage* storage, uint64_t result) {
            auto* call = std::launder(reinterpret_cast<rsf_census_call_state*>(storage));
            rsf_census_call_finish(call, result);
        },
        safe_read,
        read_arg_string,
        [](const char* name, void* target) {
            const Fn fn = lookup_name(name);
            return fn != Fn::Count && !std::strcmp(kFns[idx(fn)].api, "vulkan") ?
                bind_thunk(fn, target) : target;
        },
        [](void* queue, uint32_t family, uint32_t index) {
            queue_describe(QueueKind::Vulkan, queue, family, index, 0);
        },
        [](void* queue, uint32_t count) { queue_submit(QueueKind::Vulkan, queue, count); },
        []() { if (tl_present_depth++ == 0) note_present_begin(); },
        []() { --tl_present_depth; },
    };
    void* wrappers[RSF_CENSUS_VK_COUNT]{};
    rsf_census_vk_bind(&services, wrappers);
    for (size_t i = 0; i < std::size(entries); ++i) table[idx(entries[i])] = wrappers[i];
}

void register_wrappers_ngx(void** table) noexcept {
    static constexpr uint16_t entries[] = {
#define RSF_CENSUS_NGX_ID(name, api, symbol, modules) static_cast<uint16_t>(Fn::name),
        RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, D3D12, nullptr, nullptr)
        RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, D3D11, nullptr, nullptr)
        RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, VULKAN, nullptr, nullptr)
#undef RSF_CENSUS_NGX_ID
    };
    static_assert(std::size(entries) == RSF_CENSUS_NGX_ENTRIES);
    static const rsf_census_ngx_platform platform = {
        safe_read,
        read_arg_string,
        [](const void* address, uint16_t* text, size_t capacity) {
            return read_arg_wstring(address, reinterpret_cast<wchar_t*>(text), capacity);
        },
        [](const void* parameters, size_t slot, const char* name, void* output) {
            using Getter = int32_t (*)(const void*, const char*, void*);
            auto get = reinterpret_cast<Getter>(vtable_slot(parameters, slot));
            int32_t result = 0;
            return get && guard([&] { result = get(parameters, name, output); }) && result == 1;
        },
        [](rsf_census_json* state, const char* key, const void* resource, bool d3d12) {
            Json json(state);
            if (d3d12) json_d3d12_resource(json, key, resource);
            else json_d3d11_resource(json, key, resource);
        },
        [](rsf_census_json* state, const char* key, double value) {
            Json(state).f32(key, value);
        },
    };
    void* wrappers[RSF_CENSUS_NGX_ENTRIES]{};
    rsf_census_ngx_bind(&platform, entries, wrappers);
    for (size_t i = 0; i < std::size(entries); ++i) table[entries[i]] = wrappers[i];
}

void Json::f32(const char* key, double value) noexcept {
    if (!std::isfinite(value)) {
        null(key);
        return;
    }
    char text[40];
    std::snprintf(text, sizeof(text), "%.9g", value);
    raw(key, text);
}

void note_present_begin() noexcept {
    if (installed()) rsf_census_stats_present();
}

uint64_t present_index() noexcept { return rsf_census_stats_present_index(); }

void queue_describe(QueueKind kind, const void* queue, uint32_t a, uint32_t b, uint32_t c) noexcept {
    if (installed()) rsf_census_stats_queue_describe(static_cast<uint8_t>(kind), queue, a, b, c);
}

bool queue_submit(QueueKind kind, const void* queue, uint32_t lists) noexcept {
    return installed() && rsf_census_stats_queue_submit(static_cast<uint8_t>(kind), queue, lists);
}

} // namespace rsf_census

extern "C" rsf_census_result rsf_census_install(const rsf_census_options* options) {
    using namespace rsf_census;
    static const rsf_census_core_platform platform = {
        kFns, static_cast<uint16_t>(kFnCount), static_cast<uint16_t>(Fn::CensusEvent), hooks_install,
    };
    return rsf_census_install_impl(options, &platform);
}
