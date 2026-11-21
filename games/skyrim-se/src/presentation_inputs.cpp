// SPDX-License-Identifier: MIT
#include "presentation_inputs.h"
#include "cpu_events.h"
#include "render_camera.h"
#include <rescaleframe/d3d11_host.h>
#include <MinHook.h>
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <atomic>
#include <cstring>
#include <initializer_list>

namespace rsf::skyrim {
namespace {
using Microsoft::WRL::ComPtr;
using DrawInterface = uintptr_t (*)(void*);
using Capture = decltype(&rsf_d3d11_host_capture_texture);
DrawInterface original{};
Capture capture{};
rsf_game_host_services host{};
std::atomic<bool> live{};
std::atomic<unsigned> calls{};
std::atomic<bool> pending{};
struct Identity {
    uint64_t session{}, frame{}, view{};
    uintptr_t chain{};
    uint32_t width{}, height{};
};
thread_local Identity identity;
thread_local rsf_game_render_pass scene_identity;
thread_local rsf_game_render_pass window_pass;
thread_local Microsoft::WRL::ComPtr<IDXGISwapChain> window_chain;
thread_local Microsoft::WRL::ComPtr<ID3D11DeviceContext> window_context;
bool read(void* out, uintptr_t address, size_t size) noexcept {
#if defined(_MSC_VER)
    __try { std::memcpy(out, reinterpret_cast<const void*>(address), size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION m{};
    if (!VirtualQuery(reinterpret_cast<void*>(address), &m, sizeof(m)) || m.State != MEM_COMMIT ||
        (m.Protect & (PAGE_NOACCESS | PAGE_GUARD)) || address + size > uintptr_t(m.BaseAddress) + m.RegionSize) return false;
    std::memcpy(out, reinterpret_cast<const void*>(address), size); return true;
#endif
}
template<class T> bool read(T& out, uintptr_t address) { return read(&out, address, sizeof(out)); }
void* target() { return reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0x116ac80; }

void snapshot(bool after) {
    if (!live.load(std::memory_order_acquire) || !identity.session) return;
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    uint32_t frame{}; uintptr_t window{}; IDXGISwapChain* raw_chain{}; ID3D11DeviceContext* context{};
    if (!read(frame, base + 0x33363f4) || identity.frame != frame ||
        identity.session != host.session_id) return;
    if (!read(window, base + 0x3330198) || !window ||
        !read(raw_chain, window + 0x18) || !raw_chain || uintptr_t(raw_chain) != identity.chain ||
        !read(context, base + 0x3331f30) || !context) return;
    Microsoft::WRL::ComPtr<IDXGISwapChain> chain = raw_chain;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> buffer;
    if (FAILED(chain->GetBuffer(0, IID_PPV_ARGS(&buffer)))) return;
    D3D11_TEXTURE2D_DESC desc{}; buffer->GetDesc(&desc);
    if (desc.Width != identity.width || desc.Height != identity.height || desc.SampleDesc.Count != 1) return;
    rsf_game_render_pass current{};
    const bool same_scope = cpu_render_identity(current) &&
        current.source_frame_id == scene_identity.source_frame_id &&
        current.submission_id == scene_identity.submission_id && current.scope_id == scene_identity.scope_id;
    if (!after && same_scope && host.render_pass && scene_identity.source_frame_id && !window_chain) {
        auto final = scene_identity;
        // A later native refusal must survive even if inputs were retained during
        // gameplay. Only the original producer can grant admission for this frame.
        if (current.screen != final.screen || (current.flags & RSF_GAME_RENDER_NO_FG)) {
            final.flags |= RSF_GAME_RENDER_NO_FG;
            final.screen = current.screen;
        }
        final.role = RSF_GAME_RENDER_FINAL_SCENE; final.scene_surface = buffer.Get();
        final.swapchain = chain.Get();
        host.render_pass(host.user, context, &final, 1);
        host.render_pass(host.user, context, &final, 0);
        window_chain = chain; window_context = context;
        window_pass = final; window_pass.role = RSF_GAME_RENDER_WINDOW;
        window_pass.scene_surface = nullptr;
        window_pass.swapchain = window_chain.Get();
        pending.store(true, std::memory_order_release);
        cpu_render_diagnostic("WINDOW", window_pass);
        host.render_pass(host.user, context, &window_pass, 1);
    }
    const rsf_d3d11_host_capture request{
        sizeof(request), RSF_D3D11_HOST_ABI_VERSION, identity.session, identity.frame, identity.view,
        after ? "native_after_interface" : "native_before_interface",
        after ? "Native backbuffer after interface drawing; later native output work may remain" :
            "Native backbuffer before interface drawing; candidate scene boundary, not yet FG eligible",
        buffer.Get(), 0, 1};
    // The runtime rejects unarmed or mismatched captures before readback. No GPU
    // resource from this native point survives the synchronous capture call.
    if (capture) (void)capture(host.user, context, &request);
}
uintptr_t observe(void* ui) noexcept {
    calls.fetch_add(1, std::memory_order_acq_rel);
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    uintptr_t primary_ui{};
    const bool primary = read(primary_ui, base + 0x219e5c0) && uintptr_t(ui) == primary_ui;
    if (primary) { try { snapshot(false); } catch (...) {} }
    const auto result = original(ui);
    if (primary) { try { snapshot(true); } catch (...) {} }
    calls.fetch_sub(1, std::memory_order_acq_rel);
    return result;
}
}
bool prepare_presentation_inputs(void* manager, const rsf_game_host_services& services) {
    if (window_chain) return false;
    identity = {}; scene_identity = {};
    if (!services.render_pass || !live.load(std::memory_order_acquire) ||
        services.session_id != host.session_id || services.user != host.user) return false;
    rsf_game_render_pass pass{};
    RenderCamera view{};
    if (!read_render_camera(manager, true, pass, view)) return false;
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    const auto [width, height] = view.output;
    const auto [render_width, render_height] = view.render;
    const auto camera = view.camera, window = view.window;
    const auto frame = view.frame;
    ID3D11DeviceContext* raw_context{}; ID3D11Texture2D *raw_color{}, *raw_depth{}, *raw_motion{}; IDXGISwapChain* raw_chain{};
    if (!read(raw_context, base + 0x3331f30) || !raw_context ||
        !read(raw_color, base + 0x3331f40 + 0xa58 + 0x30) || !raw_color ||
        !read(raw_motion, base + 0x3331f40 + 0xa58 + 7 * 0x30) || !raw_motion ||
        !read(raw_depth, base + 0x3333f58) || !raw_depth || !read(raw_chain, window + 0x18) || !raw_chain) return false;
    ComPtr<ID3D11DeviceContext> context = raw_context;
    ComPtr<ID3D11Texture2D> color = raw_color, depth = raw_depth, motion = raw_motion;
    ComPtr<IDXGISwapChain> chain = raw_chain;
    if (context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return false;
    ComPtr<ID3D11Device> device, owner; context->GetDevice(&device);
    for (auto* texture : {color.Get(), depth.Get(), motion.Get()}) {
        texture->GetDevice(&owner); if (owner.Get() != device.Get()) return false;
        D3D11_TEXTURE2D_DESC d{}; texture->GetDesc(&d);
        if (d.Width != width || d.Height != height || d.SampleDesc.Count != 1 || d.ArraySize != 1 || d.MipLevels != 1) return false;
    }
    if (FAILED(chain->GetDevice(IID_PPV_ARGS(&owner))) || owner.Get() != device.Get()) return false;
    D3D11_TEXTURE2D_DESC c{}, d{}, m{}; color->GetDesc(&c); depth->GetDesc(&d); motion->GetDesc(&m);
    if (c.Format != DXGI_FORMAT_R11G11B10_FLOAT || d.Format != DXGI_FORMAT_R24G8_TYPELESS || m.Format != DXGI_FORMAT_R16G16_FLOAT) return false;
    pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_FG_INPUTS; pass.session_id = services.session_id;
    pass.family_key = pass.view_key = pass.history_key = camera; pass.pass_key = 0x153ade7; pass.native_frame = frame;
    pass.flags |= RSF_GAME_RENDER_PRIMARY;
    if (!cpu_render_identity(pass) || !pass.source_frame_id || !pass.submission_id || !pass.scope_id) return false;
    // The native World observer owns this fact. A camera/backbuffer alone cannot
    // establish gameplay, input timing or generation eligibility.
    if (!cpu_rendered_source_eligible() || pass.screen != RSF_SCREEN_GAMEPLAY)
        pass.flags |= RSF_GAME_RENDER_NO_FG;
    pass.viewport_key = pass.window_key = pass.rhi_viewport_key = window;
    pass.render_rect[2] = int32_t(render_width); pass.render_rect[3] = int32_t(render_height);
    pass.output_rect[2] = int32_t(width); pass.output_rect[3] = int32_t(height);
    pass.color_input = pass.color_output = color.Get(); pass.depth = depth.Get(); pass.motion = motion.Get();
    pass.swapchain = chain.Get(); pass.camera_valid = 1;
    pass.motion_to_uv[0] = pass.motion_to_uv[1] = -1; pass.motion_camera_included = 1;
    // The runtime normalizes and retains native inputs synchronously here, before
    // SR auxiliary reconstruction or native post-processing can overwrite them.
    // AddRef alone would preserve the allocation, not these frame contents.
    rsf_game_render_pass current{};
    uint32_t current_frame{};
    if (!read(current_frame, base + 0x33363f4) || current_frame != frame ||
        !cpu_render_identity(current) || current.source_frame_id != pass.source_frame_id ||
        current.submission_id != pass.submission_id || current.scope_id != pass.scope_id) return false;
    cpu_render_diagnostic("FG_INPUTS", pass);
    services.render_pass(services.user, context.Get(), &pass, 1);
    services.render_pass(services.user, context.Get(), &pass, 0);
    identify_presentation_inputs(pass);
    return true;
}
bool presentation_inputs_guard() {
    constexpr unsigned char expected[]{0x4c,0x8b,0xdc,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x57,0x48,0x83,0xec,0x50};
    unsigned char actual[sizeof(expected)]{};
    return read(actual, uintptr_t(target())) && std::memcmp(actual, expected, sizeof(expected)) == 0;
}
bool presentation_inputs_start(const rsf_game_host_services& services) {
    if (calls.load(std::memory_order_acquire)) return false;
    if (!original && (!presentation_inputs_guard() ||
        MH_CreateHook(target(), reinterpret_cast<void*>(&observe), reinterpret_cast<void**>(&original)) != MH_OK)) return false;
    if (!live.load(std::memory_order_acquire) && !presentation_inputs_guard()) return false;
    capture = reinterpret_cast<Capture>(reinterpret_cast<void*>(GetProcAddress(
        GetModuleHandleW(L"ReScaleFrame.Runtime.dll"), "rsf_d3d11_host_capture_texture")));
    host = services;
    const auto result = MH_EnableHook(target());
    if (result != MH_OK && result != MH_ERROR_ENABLED) return false;
    live.store(true, std::memory_order_release); return true;
}
void presentation_inputs_quiesce() { live.store(false, std::memory_order_release); }
bool presentation_inputs_busy() { return pending.load(std::memory_order_acquire) || calls.load(std::memory_order_acquire) != 0; }
bool presentation_inputs_stop() {
    presentation_inputs_quiesce();
    if (!original) return true;
    const auto result = MH_DisableHook(target());
    return (result == MH_OK || result == MH_ERROR_DISABLED) && !presentation_inputs_busy();
}
void identify_presentation_inputs(const rsf_game_render_pass& pass) {
    rsf_game_render_pass current{};
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    uint32_t frame{};
    if (pass.session_id != host.session_id || !read(frame, base + 0x33363f4) || frame != pass.native_frame) return;
    if (pass.source_frame_id && (!cpu_render_identity(current) ||
        current.source_frame_id != pass.source_frame_id || current.submission_id != pass.submission_id ||
        current.scope_id != pass.scope_id)) return;
    // A selected frame cannot be rebound to another source or execution scope.
    if (scene_identity.session_id == pass.session_id && scene_identity.native_frame == pass.native_frame &&
        scene_identity.session_id && (scene_identity.source_frame_id != pass.source_frame_id ||
        scene_identity.submission_id != pass.submission_id || scene_identity.scope_id != pass.scope_id)) return;
    // Preserve the metadata paired with the pre-mutation FG input copy when the
    // later SR callback reports this same execution again.
    if (scene_identity.role == RSF_GAME_RENDER_FG_INPUTS &&
        scene_identity.session_id == pass.session_id && scene_identity.native_frame == pass.native_frame &&
        scene_identity.view_key == pass.view_key && scene_identity.source_frame_id == pass.source_frame_id &&
        scene_identity.submission_id == pass.submission_id && scene_identity.scope_id == pass.scope_id) return;
    identity = {pass.session_id, pass.native_frame, pass.view_key, uintptr_t(pass.swapchain),
        pass.camera.output_width, pass.camera.output_height};
    // Copy scalar engine identity and camera only. No borrowed SR resources survive.
    scene_identity = {};
    scene_identity.struct_size = sizeof(scene_identity); scene_identity.role = pass.role;
    scene_identity.session_id = pass.session_id; scene_identity.source_frame_id = pass.source_frame_id;
    scene_identity.submission_id = pass.submission_id; scene_identity.scope_id = pass.scope_id;
    scene_identity.family_key = pass.family_key; scene_identity.view_key = pass.view_key;
    scene_identity.history_key = pass.history_key; scene_identity.native_frame = pass.native_frame;
    scene_identity.pass_key = pass.pass_key; scene_identity.resource_generation = pass.resource_generation;
    scene_identity.viewport_key = pass.viewport_key; scene_identity.window_key = pass.window_key;
    scene_identity.rhi_viewport_key = pass.rhi_viewport_key; scene_identity.flags = pass.flags;
    if (pass.role != RSF_GAME_RENDER_FG_INPUTS) scene_identity.flags |= RSF_GAME_RENDER_NO_FG;
    scene_identity.screen = pass.screen;
    scene_identity.camera = pass.camera; scene_identity.camera_valid = pass.camera_valid;
    scene_identity.motion_camera_included = pass.motion_camera_included;
    std::memcpy(scene_identity.motion_to_uv, pass.motion_to_uv, sizeof(pass.motion_to_uv));
    std::memcpy(scene_identity.previous_clip_to_clip, pass.previous_clip_to_clip, sizeof(pass.previous_clip_to_clip));
    std::memcpy(scene_identity.render_rect, pass.render_rect, sizeof(pass.render_rect));
    std::memcpy(scene_identity.output_rect, pass.output_rect, sizeof(pass.output_rect));
}
void finish_presentation_inputs() {
    if (!window_chain) { identity = {}; scene_identity = {}; return; }
    if (host.render_pass) {
        try { host.render_pass(host.user, window_context.Get(), &window_pass, 0); } catch (...) {}
    }
    window_pass = {}; window_chain.Reset(); window_context.Reset();
    identity = {}; scene_identity = {};
    pending.store(false, std::memory_order_release);
}
}
