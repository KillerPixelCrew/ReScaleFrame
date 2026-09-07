/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/ui_identify.h>

#include <cstring>
#include <new>

namespace {

/* Fixed capacity per set, so nothing here allocates after creation. The caller runs inside the
   game's own resource creation, where an allocation is a lock in someone else's allocator. */
constexpr uint32_t capacity_for(rsf_ui_set set)
{
    switch (set) {
    case RSF_UI_SET_SLATE_LAYOUT:
    case RSF_UI_SET_CANVAS_LAYOUT:
        return RSF_UI_MAX_LAYOUTS;
    case RSF_UI_SET_WIDGET_TARGET:
        return RSF_UI_MAX_WIDGET_TARGETS;
    default:
        return RSF_UI_MAX_NAMED_SHADERS;
    }
}

struct Set {
    void* entries[RSF_UI_MAX_WIDGET_TARGETS]{};
    uint32_t count = 0;
};

/* Castagnoli, reflected, table free. A shader blob is hashed once at creation, so the loop is not
   worth a 1 KiB table that would sit in cache the game wants. */
uint32_t crc32c(const uint8_t* data, uint32_t bytes)
{
    uint32_t crc = 0xFFFFFFFFu;
    for (uint32_t index = 0; index < bytes; ++index) {
        crc ^= data[index];
        for (int bit = 0; bit < 8; ++bit) {
            const uint32_t mask = static_cast<uint32_t>(-static_cast<int32_t>(crc & 1u));
            crc = (crc >> 1) ^ (0x82F63B78u & mask);
        }
    }
    return ~crc;
}

} // namespace

struct rsf_ui_registry {
    Set sets[RSF_UI_SET_COUNT];
    rsf_ui_registry_counters counters{};
};

extern "C" rsf_ui_registry* rsf_ui_registry_create(uint32_t abi_version)
{
    if (abi_version != RSF_UI_IDENTIFY_ABI_VERSION) {
        return nullptr;
    }
    rsf_ui_registry* registry = new (std::nothrow) rsf_ui_registry();
    if (registry) {
        registry->counters.struct_size = sizeof(rsf_ui_registry_counters);
    }
    return registry;
}

extern "C" void rsf_ui_registry_destroy(rsf_ui_registry* registry) { delete registry; }

extern "C" rsf_ui_result rsf_ui_registry_add(rsf_ui_registry* registry, rsf_ui_set set,
                                             void* object)
{
    if (!registry || !object || set >= RSF_UI_SET_COUNT) {
        return RSF_UI_ERROR_INVALID_ARGUMENT;
    }
    Set& target = registry->sets[set];
    for (uint32_t index = 0; index < target.count; ++index) {
        if (target.entries[index] == object) {
            return RSF_UI_OK;
        }
    }
    if (target.count >= capacity_for(set)) {
        ++registry->counters.refused_full[set];
        return RSF_UI_ERROR_FULL;
    }
    target.entries[target.count++] = object;
    ++registry->counters.recorded[set];
    return RSF_UI_OK;
}

extern "C" void rsf_ui_registry_forget(rsf_ui_registry* registry, void* object)
{
    if (!registry || !object) {
        return;
    }
    for (uint32_t set = 0; set < RSF_UI_SET_COUNT; ++set) {
        Set& target = registry->sets[set];
        for (uint32_t index = 0; index < target.count; ++index) {
            if (target.entries[index] != object) {
                continue;
            }
            /* Order does not carry meaning in a membership set, so the last entry fills the hole
               and the scan stays linear. */
            target.entries[index] = target.entries[target.count - 1];
            target.entries[target.count - 1] = nullptr;
            --target.count;
            ++registry->counters.forgotten_on_reuse;
            break;
        }
    }
}

extern "C" int rsf_ui_registry_contains(const rsf_ui_registry* registry, rsf_ui_set set,
                                        const void* object)
{
    if (!registry || !object || set >= RSF_UI_SET_COUNT) {
        return 0;
    }
    const Set& target = registry->sets[set];
    for (uint32_t index = 0; index < target.count; ++index) {
        if (target.entries[index] == object) {
            return 1;
        }
    }
    return 0;
}

extern "C" void* const* rsf_ui_registry_view(const rsf_ui_registry* registry, rsf_ui_set set,
                                             uint32_t* count_out)
{
    if (!registry || set >= RSF_UI_SET_COUNT) {
        if (count_out) {
            *count_out = 0;
        }
        return nullptr;
    }
    if (count_out) {
        *count_out = registry->sets[set].count;
    }
    return registry->sets[set].entries;
}

extern "C" rsf_ui_result rsf_ui_registry_get_counters(const rsf_ui_registry* registry,
                                                      rsf_ui_registry_counters* counters)
{
    if (!registry || !counters || counters->struct_size < sizeof(rsf_ui_registry_counters)) {
        return RSF_UI_ERROR_INVALID_ARGUMENT;
    }
    const uint32_t size = counters->struct_size;
    *counters = registry->counters;
    counters->struct_size = size;
    return RSF_UI_OK;
}

extern "C" uint32_t rsf_ui_shader_hash(const void* bytecode, uint32_t bytes)
{
    if (!bytecode || bytes == 0) {
        return 0;
    }
    return crc32c(static_cast<const uint8_t*>(bytecode), bytes);
}
