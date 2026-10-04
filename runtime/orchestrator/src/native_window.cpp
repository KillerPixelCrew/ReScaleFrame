// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_window.h>
#include <rescaleframe/native_scene.h>
#include <unknwn.h>
#include <windows.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <array>
namespace {
thread_local std::array<rsf_game_render_pass, 32> scopes{};
thread_local uint32_t count = 0, untracked = 0;
thread_local rsf_native_present_record last_present{};
thread_local bool presenting = false;
}
extern "C" RSF_RUNTIME_API int rsf_native_window_scope(const rsf_game_render_pass* pass, uint32_t begin)
{
    if (!pass || pass->struct_size < sizeof(*pass) || pass->role != RSF_GAME_RENDER_WINDOW ||
        !pass->session_id || !pass->source_frame_id || !pass->scope_id || !pass->window_key ||
        !pass->viewport_key || !pass->rhi_viewport_key || !pass->swapchain) return 0;
    if (begin) {
        if (untracked || count == scopes.size()) { ++untracked; return 0; }
        scopes[count++] = *pass; return 1;
    }
    if (untracked) { --untracked; return 0; }
    if (!count) return 0;
    const auto& current = scopes[count - 1];
    if (current.session_id != pass->session_id || current.scope_id != pass->scope_id ||
        current.source_frame_id != pass->source_frame_id || current.window_key != pass->window_key ||
        current.rhi_viewport_key != pass->rhi_viewport_key || current.viewport_key != pass->viewport_key ||
        current.swapchain != pass->swapchain) {
        // The completed ticket's lease is about to retire. Never leave borrowed identities
        // reachable after a mismatched end; require a fresh begin for later presentation.
        scopes = {}; count = untracked = 0; return 0;
    }
    scopes[--count] = {}; return 1;
}
extern "C" RSF_RUNTIME_API int rsf_native_window_match(void* native_swapchain, rsf_game_render_pass* pass)
{
    if (!native_swapchain || !pass || pass->struct_size < sizeof(*pass) || !count || untracked) return 0;
    const auto& current = scopes[count - 1];
    Microsoft::WRL::ComPtr<IUnknown> observed, owned;
    if (FAILED(static_cast<IUnknown*>(native_swapchain)->QueryInterface(IID_PPV_ARGS(&observed))) ||
        FAILED(static_cast<IUnknown*>(current.swapchain)->QueryInterface(IID_PPV_ARGS(&owned))) ||
        observed.Get() != owned.Get()) return 0;
    *pass = current; return 1;
}
extern "C" RSF_RUNTIME_API int rsf_native_window_present(const rsf_observer_present_event* event)
{
    if (!event || event->struct_size < sizeof(*event) || !event->swapchain || event->completed > 1 || event->method > 1) return 0;
    if (event->flags & DXGI_PRESENT_TEST) return 0;
    LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    rsf_game_render_pass window{}; window.struct_size = sizeof(window);
    const bool matched = rsf_native_window_match(event->swapchain, &window) != 0;
    if (!event->completed) {
        last_present = {}; last_present.struct_size = sizeof(last_present);
        last_present.matched = matched; last_present.sync_interval = event->sync_interval;
        last_present.method = event->method;
        last_present.flags = event->flags; last_present.begin_qpc = uint64_t(now.QuadPart);
        last_present.qpc_frequency = uint64_t(frequency.QuadPart);
        if (matched) {
            last_present.session_id = window.session_id; last_present.source_frame_id = window.source_frame_id;
            last_present.scope_id = window.scope_id; last_present.viewport_key = window.viewport_key;
            last_present.window_key = window.window_key;
            rsf_native_scene_identity scene{}; scene.struct_size = sizeof(scene);
            if (rsf_native_scene_for_window(&window, &scene)) {
                last_present.scene_matched = 1; last_present.scene_submission_id = scene.submission_id;
                last_present.scene_texture_bound = scene.texture_bound && scene.bound_window_key == window.window_key;
                last_present.scene_direct_present = rsf_native_scene_present_path(&window) == RSF_NATIVE_SCENE_PRESENT_DIRECT;
                last_present.scene_view_key = scene.view_key; last_present.scene_native_frame = scene.native_frame;
            }
        }
        presenting = true; return 1;
    }
    if (!presenting || last_present.sync_interval != event->sync_interval || last_present.flags != event->flags || last_present.method != event->method ||
        bool(last_present.matched) != matched || (matched && (last_present.session_id != window.session_id ||
        last_present.source_frame_id != window.source_frame_id || last_present.scope_id != window.scope_id))) {
        presenting = false; last_present = {}; return 0;
    }
    if (matched && last_present.scene_matched) {
        rsf_native_scene_identity scene{}; scene.struct_size = sizeof(scene);
        if (!rsf_native_scene_for_window(&window, &scene) || scene.submission_id != last_present.scene_submission_id ||
            scene.view_key != last_present.scene_view_key || scene.native_frame != last_present.scene_native_frame)
            { last_present.scene_matched = 0; last_present.scene_texture_bound = 0; last_present.scene_direct_present = 0; }
    }
    presenting = false; last_present.result = event->result; last_present.completed = 1;
    last_present.accepted = event->result == S_OK;
    last_present.end_qpc = uint64_t(now.QuadPart);
    return 1;
}
extern "C" RSF_RUNTIME_API int rsf_native_window_last_present(rsf_native_present_record* record)
{
    if (!record || record->struct_size < sizeof(*record) || !last_present.completed) return 0;
    *record = last_present; return 1;
}
extern "C" RSF_RUNTIME_API void rsf_native_window_reset(void)
{ scopes = {}; count = untracked = 0; last_present = {}; presenting = false; }
