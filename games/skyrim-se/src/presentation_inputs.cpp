// SPDX-License-Identifier: MIT
#include "presentation_inputs.h"
#include "cpu_events.h"
#include "render_camera.h"
#include "observer_platform.h"
#include <rescaleframe/d3d11_host.h>
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <cstring>
#include <initializer_list>

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
bool rsf_skyrim_presentation_begin_inputs(const rsf_game_host_services*) noexcept;
void rsf_skyrim_presentation_identify(const rsf_game_render_pass*) noexcept;
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

bool prepare_presentation_inputs(void* manager, const rsf_game_host_services& services)
{
    if (!rsf_skyrim_presentation_begin_inputs(&services)) return false;
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
bool presentation_inputs_guard() { return rsf_skyrim_presentation_guard(creation_platform()); }
bool presentation_inputs_start(const rsf_game_host_services& services)
{
    return rsf_skyrim_presentation_start(&services, creation_platform(), &leaves, reinterpret_cast<const void*>(&observe));
}
void presentation_inputs_quiesce() { rsf_skyrim_presentation_quiesce(); }
bool presentation_inputs_busy() { return rsf_skyrim_presentation_busy(); }
bool presentation_inputs_stop() { return rsf_skyrim_presentation_stop(creation_platform()); }
void identify_presentation_inputs(const rsf_game_render_pass& pass) { rsf_skyrim_presentation_identify(&pass); }
void finish_presentation_inputs() { rsf_skyrim_presentation_finish(); }
}
