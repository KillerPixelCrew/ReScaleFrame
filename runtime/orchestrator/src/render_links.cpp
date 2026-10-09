// SPDX-License-Identifier: GPL-3.0-only
// Unwired scaffolding: no production caller yet. Kept for the planned ABI 2 frame-callback
// work (see docs/representation-plan.md). Do not delete as dead code.
#include <rescaleframe/render_links.h>
#include <limits>
#include <mutex>
#include <new>

struct rsf_render_links {
    std::mutex guard;
    uint64_t session_id = 0;
    uint64_t last_submission = 0;
    uint32_t capacity = 0;
    rsf_render_link slots[64]{};
};

namespace {
rsf_backend_result validate(const rsf_render_link* link)
{
    if (!link || link->struct_size < sizeof(*link)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return link->abi_version == RSF_RENDER_LINK_ABI_VERSION ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_ABI_MISMATCH;
}
}

extern "C" rsf_backend_result rsf_render_links_create(uint64_t session_id, uint32_t capacity,
    rsf_render_links** out) try
{
    if (!out) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    if (!session_id || !capacity || capacity > 64) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto* links = new (std::nothrow) rsf_render_links;
    if (!links) return RSF_BACKEND_ERROR_INIT_FAILED;
    links->session_id = session_id;
    links->capacity = capacity;
    *out = links;
    return RSF_BACKEND_OK;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

extern "C" rsf_backend_result rsf_render_links_bind(rsf_render_links* links, uint64_t key,
    uint64_t frame, uint64_t generation, rsf_render_link* out) try
{
    const auto result = validate(out);
    if (result != RSF_BACKEND_OK) return result;
    *out = {sizeof(*out), RSF_RENDER_LINK_ABI_VERSION, 0, 0, 0, 0, 0};
    if (!links || !key || !frame || !generation) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(links->guard);
    rsf_render_link* available = nullptr;
    for (uint32_t i = 0; i < links->capacity; ++i) {
        auto& slot = links->slots[i];
        if (slot.producer_key == key) return RSF_BACKEND_ERROR_NOT_READY;
        if (!slot.producer_key && !available) available = &slot;
    }
    if (!available || links->last_submission == std::numeric_limits<uint64_t>::max())
        return RSF_BACKEND_ERROR_NOT_READY;
    *available = {sizeof(*out), RSF_RENDER_LINK_ABI_VERSION, links->session_id,
        frame, ++links->last_submission, generation, key};
    *out = *available;
    return RSF_BACKEND_OK;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

extern "C" rsf_backend_result rsf_render_links_lookup(rsf_render_links* links, uint64_t key,
    uint64_t generation, rsf_render_link* out) try
{
    const auto result = validate(out);
    if (result != RSF_BACKEND_OK) return result;
    *out = {sizeof(*out), RSF_RENDER_LINK_ABI_VERSION, 0, 0, 0, 0, 0};
    if (!links || !key || !generation) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(links->guard);
    for (uint32_t i = 0; i < links->capacity; ++i) {
        const auto& slot = links->slots[i];
        if (slot.producer_key != key) continue;
        if (slot.resource_generation != generation) return RSF_BACKEND_ERROR_STALE_RESOURCES;
        *out = slot;
        return RSF_BACKEND_OK;
    }
    return RSF_BACKEND_ERROR_NOT_READY;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

extern "C" rsf_backend_result rsf_render_links_release(rsf_render_links* links,
    const rsf_render_link* link) try
{
    const auto result = validate(link);
    if (result != RSF_BACKEND_OK) return result;
    if (!links || !link->producer_key || !link->submission_id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(links->guard);
    for (uint32_t i = 0; i < links->capacity; ++i) {
        auto& slot = links->slots[i];
        if (slot.producer_key != link->producer_key) continue;
        if (slot.submission_id != link->submission_id || slot.session_id != link->session_id ||
            slot.source_frame_id != link->source_frame_id || slot.resource_generation != link->resource_generation)
            return RSF_BACKEND_ERROR_STALE_RESOURCES;
        slot = {};
        return RSF_BACKEND_OK;
    }
    return RSF_BACKEND_ERROR_NOT_READY;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

extern "C" rsf_backend_result rsf_render_links_destroy(rsf_render_links* links) try
{
    if (!links) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    {
        std::lock_guard<std::mutex> lock(links->guard);
        for (uint32_t i = 0; i < links->capacity; ++i)
            if (links->slots[i].producer_key) return RSF_BACKEND_ERROR_NOT_READY;
    }
    delete links;
    return RSF_BACKEND_OK;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }
