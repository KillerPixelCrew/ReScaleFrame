// SPDX-License-Identifier: MIT
#include "presentation_inputs.h"
#include "cpu_events.h"
#include "observer_platform.h"
#include <rescaleframe/d3d11_host.h>
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>

extern "C" {
struct rsf_skyrim_presentation_buffer {
    void* chain;
    void* context;
    void* texture;
    uint32_t width, height, samples;
};
using BufferCallback = void (*)(const void*, const rsf_skyrim_presentation_buffer*);
struct rsf_skyrim_presentation_leaves {
    bool (*identity)(rsf_game_render_pass*);
    bool (*render)(const rsf_game_host_services*, void*, const rsf_game_render_pass*, uint32_t);
    void (*diagnostic)(const char*, const rsf_game_render_pass*);
    void (*buffer)(void*, void*, const void*, BufferCallback);
    void (*retain)(void*, void*);
    void (*release)(void*, void*);
    void (*capture)(uintptr_t, void*, void*, const rsf_d3d11_host_capture*);
};
bool rsf_skyrim_presentation_guard(const rsf_skyrim_creation_platform*) noexcept;
bool rsf_skyrim_presentation_start(const rsf_game_host_services*, const rsf_skyrim_creation_platform*,
    const rsf_skyrim_presentation_leaves*, const void* detour) noexcept;
void rsf_skyrim_presentation_quiesce() noexcept;
bool rsf_skyrim_presentation_busy() noexcept;
bool rsf_skyrim_presentation_stop(const rsf_skyrim_creation_platform*) noexcept;
void rsf_skyrim_presentation_finish() noexcept;
void* rsf_skyrim_presentation_enter(uintptr_t ui, bool* primary) noexcept;
void rsf_skyrim_presentation_leave(bool primary) noexcept;
}

static_assert(sizeof(rsf_skyrim_presentation_buffer) == 40 && sizeof(rsf_skyrim_presentation_leaves) == 56);
static_assert(sizeof(rsf_d3d11_host_capture) == 64 && offsetof(rsf_d3d11_host_capture, texture) == 48);

namespace rsf::skyrim {
namespace {
using Microsoft::WRL::ComPtr;
using DrawInterface = uintptr_t (*)(void*);
using Capture = decltype(&rsf_d3d11_host_capture_texture);

uintptr_t observe(void* ui) noexcept
{
    struct Call {
        bool primary{};
        ~Call() { rsf_skyrim_presentation_leave(primary); }
    } call;
    const auto original = reinterpret_cast<DrawInterface>(rsf_skyrim_presentation_enter(uintptr_t(ui), &call.primary));
    return original ? original(ui) : 0;
}

constexpr rsf_skyrim_presentation_leaves leaves{
    [](rsf_game_render_pass* pass) noexcept {
        try { return cpu_render_identity(*pass); } catch (...) { return false; }
    },
    [](const rsf_game_host_services* host, void* context, const rsf_game_render_pass* pass, uint32_t begin) noexcept {
        if (!host->render_pass) return false;
        try { host->render_pass(host->user, context, pass, begin); return true; } catch (...) { return false; }
    },
    [](const char* stage, const rsf_game_render_pass* pass) noexcept {
        try { cpu_render_diagnostic(stage, *pass); } catch (...) {}
    },
    [](void* raw_chain, void* raw_context, const void* user, BufferCallback callback) noexcept {
        ComPtr<IDXGISwapChain> chain = static_cast<IDXGISwapChain*>(raw_chain);
        ComPtr<ID3D11DeviceContext> context = static_cast<ID3D11DeviceContext*>(raw_context);
        ComPtr<ID3D11Texture2D> texture;
        if (FAILED(chain->GetBuffer(0, IID_PPV_ARGS(&texture)))) return;
        D3D11_TEXTURE2D_DESC desc{};
        texture->GetDesc(&desc);
        const rsf_skyrim_presentation_buffer facts{
            chain.Get(), context.Get(), texture.Get(), desc.Width, desc.Height, desc.SampleDesc.Count
        };
        callback(user, &facts);
    },
    [](void* chain, void* context) noexcept {
        static_cast<IDXGISwapChain*>(chain)->AddRef();
        static_cast<ID3D11DeviceContext*>(context)->AddRef();
    },
    [](void* chain, void* context) noexcept {
        static_cast<IDXGISwapChain*>(chain)->Release();
        static_cast<ID3D11DeviceContext*>(context)->Release();
    },
    [](uintptr_t service, void* user, void* context, const rsf_d3d11_host_capture* request) noexcept {
        try { (void)reinterpret_cast<Capture>(service)(user, context, request); } catch (...) {}
    },
};
}

bool presentation_inputs_guard() { return rsf_skyrim_presentation_guard(creation_platform()); }
bool presentation_inputs_start(const rsf_game_host_services& services)
{
    return rsf_skyrim_presentation_start(&services, creation_platform(), &leaves, reinterpret_cast<const void*>(&observe));
}
void presentation_inputs_quiesce() { rsf_skyrim_presentation_quiesce(); }
bool presentation_inputs_busy() { return rsf_skyrim_presentation_busy(); }
bool presentation_inputs_stop() { return rsf_skyrim_presentation_stop(creation_platform()); }
void finish_presentation_inputs() { rsf_skyrim_presentation_finish(); }
}
