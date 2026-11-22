// SPDX-License-Identifier: MIT
// Link the package's Runtime presenter to the Rust Editor window owner.
#include "preview.h"
#include <rescaleframe/d3d12_window_presenter.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <cstddef>
#include <mutex>

static_assert(sizeof(rsf_d3d12_window_target) == 24);
static_assert(offsetof(rsf_d3d12_window_target, generation) == 16);

extern "C" bool rsf_unity_preview_target(rsf_d3d12_window_target*);
extern "C" void rsf_unity_preview_show(uint64_t generation);

namespace {
std::mutex output_mutex;
rsf_d3d12_window_presenter* output{};

int observe_window(const rsf_game_render_pass* pass) noexcept try
{
    rsf_fg12_window(pass);
    return 1;
} catch (...) { return 0; }
}

bool rsf_unity_preview_prepare(void* device, void* queue, uint32_t width, uint32_t height)
{
    std::unique_lock lock(output_mutex, std::try_to_lock);
    rsf_d3d12_window_target target{};
    if (!lock.owns_lock() || !rsf_unity_preview_target(&target)) return false;
    if (!output) output = rsf_d3d12_window_create(observe_window);
    return rsf_d3d12_window_prepare(output, &target, device, queue, width, height) != 0;
}

bool rsf_unity_preview_present(void* texture, const rsf_game_render_pass& pass, bool common_state)
{
    std::unique_lock lock(output_mutex, std::try_to_lock);
    rsf_d3d12_window_target target{};
    if (!lock.owns_lock() || !output || !rsf_unity_preview_target(&target) ||
        !rsf_d3d12_window_present(output, &target, texture, &pass, common_state ? 1u : 0u)) return false;
    rsf_unity_preview_show(target.generation);
    return true;
}

bool rsf_unity_preview_drained() noexcept try
{
    std::unique_lock lock(output_mutex, std::try_to_lock);
    return lock.owns_lock() && rsf_d3d12_window_drained(output) != 0;
} catch (...) { return false; }

void rsf_unity_preview_release() noexcept
{
    // Package retirement has already closed every render route and obtained a drain proof.
    std::lock_guard lock(output_mutex);
    if (rsf_d3d12_window_destroy(output)) output = nullptr;
}
