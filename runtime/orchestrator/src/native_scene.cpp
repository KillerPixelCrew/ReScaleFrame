// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_scene.h>
#include <array>
#include <cstring>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
namespace {
thread_local std::array<rsf_native_scene_identity, 64> records{};
uint64_t resource_identity(void* resource)
{
    Microsoft::WRL::ComPtr<IUnknown> identity;
    if (!resource || FAILED(static_cast<IUnknown*>(resource)->QueryInterface(IID_PPV_ARGS(&identity)))) return 0;
    return uint64_t(reinterpret_cast<uintptr_t>(identity.Get()));
}
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
        record.surface_key = resource_identity(pass->scene_surface);
        std::memcpy(record.output_rect, pass->output_rect, sizeof(record.output_rect));
        return 1;
    }
    if (!same || record.ambiguous) return 0;
    if (record.scope_id != pass->scope_id || record.submission_id != pass->submission_id ||
        record.family_key != pass->family_key || record.view_key != pass->view_key ||
        record.viewport_key != pass->viewport_key || record.native_frame != pass->native_frame ||
        std::memcmp(record.output_rect, pass->output_rect, sizeof(record.output_rect)) ||
        !record.surface_key || record.surface_key != resource_identity(pass->scene_surface)) {
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
        record.surface_key != resource_identity(pass->sampled_texture)) return 0;
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
extern "C" RSF_RUNTIME_API uint32_t rsf_native_scene_present_path(const rsf_game_render_pass* window)
{
    rsf_native_scene_identity scene{}; scene.struct_size = sizeof(scene);
    if (!rsf_native_scene_for_window(window, &scene)) return RSF_NATIVE_SCENE_PRESENT_NONE;
    if (scene.texture_bound && scene.bound_window_key == window->window_key) return RSF_NATIVE_SCENE_PRESENT_SAMPLED;
    if (!window->swapchain || !scene.surface_key) return RSF_NATIVE_SCENE_PRESENT_NONE;
    // FSceneViewport can render straight into GetViewportBackBuffer. Slate then draws its UI
    // over that resource without ever sampling the scene through FSlateElementPS::SetTexture.
    // Compare live borrowed COM identity with the leased final-pass identity, never the address
    // of an intermediate render target or a latest-frame guess.
    Microsoft::WRL::ComPtr<ID3D11Texture2D> buffer;
    if (FAILED(static_cast<IDXGISwapChain*>(window->swapchain)->GetBuffer(0, IID_PPV_ARGS(&buffer))) ||
        resource_identity(buffer.Get()) != scene.surface_key) return RSF_NATIVE_SCENE_PRESENT_NONE;
    D3D11_TEXTURE2D_DESC desc{}; buffer->GetDesc(&desc);
    if (scene.output_rect[0] || scene.output_rect[1] || scene.output_rect[2] != int32_t(desc.Width) ||
        scene.output_rect[3] != int32_t(desc.Height)) return RSF_NATIVE_SCENE_PRESENT_NONE;
    return RSF_NATIVE_SCENE_PRESENT_DIRECT;
}
extern "C" RSF_RUNTIME_API void rsf_native_scene_reset(void)
{ records = {}; }
