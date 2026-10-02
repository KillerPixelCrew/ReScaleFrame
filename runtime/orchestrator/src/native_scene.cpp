// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_scene.h>
#include <array>
#include <cstring>
namespace {
thread_local std::array<rsf_native_scene_identity, 64> records{};
}
extern "C" RSF_RUNTIME_API int rsf_native_scene_pass(const rsf_game_render_pass* pass, uint32_t begin)
{
    if (!pass || pass->struct_size < sizeof(*pass) || pass->role != RSF_GAME_RENDER_FINAL_SCENE ||
        !(pass->flags & RSF_GAME_RENDER_PRIMARY) || !(pass->flags & RSF_GAME_RENDER_AFTER_SIMULATION) ||
        !pass->session_id || !pass->source_frame_id || !pass->viewport_key || !pass->submission_id ||
        !pass->view_key || !pass->scope_id) return 0;
    auto& record = records[pass->source_frame_id % records.size()];
    const bool same = record.session_id == pass->session_id && record.source_frame_id == pass->source_frame_id;
    if (begin) {
        if (same) {
            // A second declared primary view/composition in one source frame is ambiguous.
            record.ambiguous = 1; return 0;
        }
        if (record.source_frame_id && !record.complete && !record.ambiguous) return 0;
        record = {}; record.struct_size = sizeof(record); record.session_id = pass->session_id;
        record.source_frame_id = pass->source_frame_id; record.viewport_key = pass->viewport_key;
        record.submission_id = pass->submission_id; record.family_key = pass->family_key;
        record.view_key = pass->view_key; record.native_frame = pass->native_frame;
        record.scope_id = pass->scope_id; record.flags = pass->flags;
        record.surface_key = uint64_t(reinterpret_cast<uintptr_t>(pass->scene_surface));
        std::memcpy(record.output_rect, pass->output_rect, sizeof(record.output_rect));
        return 1;
    }
    if (!same || record.ambiguous) return 0;
    if (record.scope_id != pass->scope_id || record.submission_id != pass->submission_id ||
        record.family_key != pass->family_key || record.view_key != pass->view_key ||
        record.viewport_key != pass->viewport_key || record.native_frame != pass->native_frame ||
        std::memcmp(record.output_rect, pass->output_rect, sizeof(record.output_rect)) ||
        !pass->scene_surface || record.surface_key != uint64_t(reinterpret_cast<uintptr_t>(pass->scene_surface))) {
        record.ambiguous = 1; return 0;
    }
    record.complete = 1; return 1;
}
extern "C" RSF_RUNTIME_API int rsf_native_scene_texture_binding(const rsf_game_render_pass* pass)
{
    if (!pass || pass->struct_size < sizeof(*pass) || pass->role != RSF_GAME_RENDER_TEXTURE_BINDING ||
        !pass->sampled_texture || !pass->source_frame_id || !pass->window_key) return 0;
    auto& record = records[pass->source_frame_id % records.size()];
    if (!record.complete || record.ambiguous || record.session_id != pass->session_id ||
        record.source_frame_id != pass->source_frame_id || record.viewport_key != pass->viewport_key ||
        record.surface_key != uint64_t(reinterpret_cast<uintptr_t>(pass->sampled_texture))) return 0;
    if (record.bound_window_key && record.bound_window_key != pass->window_key) {
        record.ambiguous = 1; return 0;
    }
    record.texture_bound = 1; record.bound_window_key = pass->window_key; return 1;
}
extern "C" RSF_RUNTIME_API int rsf_native_scene_for_window(const rsf_game_render_pass* window,
    rsf_native_scene_identity* scene)
{
    if (!window || window->struct_size < sizeof(*window) || window->role != RSF_GAME_RENDER_WINDOW ||
        !scene || scene->struct_size < sizeof(*scene) || !window->session_id || !window->source_frame_id || !window->viewport_key || !window->window_key ||
        !(window->flags & RSF_GAME_RENDER_PRIMARY) || !(window->flags & RSF_GAME_RENDER_AFTER_SIMULATION)) return 0;
    const auto& record = records[window->source_frame_id % records.size()];
    if (record.session_id != window->session_id || record.source_frame_id != window->source_frame_id ||
        record.viewport_key != window->viewport_key || !record.complete || record.ambiguous) return 0;
    *scene = record; return 1;
}
extern "C" RSF_RUNTIME_API void rsf_native_scene_reset(void)
{ records = {}; }
