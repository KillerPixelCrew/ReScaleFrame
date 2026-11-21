/* SPDX-License-Identifier: MIT */
/* The caller the overlay never had. See overlay_host.h for why it lives here.

   The host's decisions belong to its Rust owner, runtime/orchestrator/src/overlay_host.rs. This
   file keeps the public entry points and the native leaves the owner calls through
   overlay_host_platform.h: the swap chain, device and output-merger COM calls, the D3D11 overlay
   renderer, the window input subclass and the Win32 clocks. */

#include <windows.h>

#include <d3d11.h>
#include <dxgi.h>

#include <rescaleframe/overlay.h>
#include <rescaleframe/overlay_input.h>
#include <rescaleframe/overlay_renderer.h>

#include "overlay_host.h"
#include "overlay_host_platform.h"

static_assert(RSF_OVERLAY_HOST_TARGET_SLOTS == D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT);
static_assert(RSF_OVERLAY_OK == 0 && RSF_OVERLAY_RENDERER_OK == 0 && RSF_OVERLAY_INPUT_OK == 0);
static_assert(VK_INSERT == 0x2D, "the Rust owner names the toggle key by value");

namespace {

uint32_t load_panel(rsf_overlay_host_panel* out, uint32_t* error)
{
    return rsf_core_overlay_panel_open(out, error, nullptr);
}

int32_t chain_device(void* chain, void** device)
{
    return static_cast<IDXGISwapChain*>(chain)->GetDevice(__uuidof(ID3D11Device), device);
}

int32_t chain_buffer(void* chain, void** texture)
{
    return static_cast<IDXGISwapChain*>(chain)->GetBuffer(0, __uuidof(ID3D11Texture2D), texture);
}

/* Asked for rather than searched for: a game process has several windows and only the swap chain
   knows which one its frames land in. */
void* chain_window(void* chain)
{
    DXGI_SWAP_CHAIN_DESC description{};
    if (FAILED(static_cast<IDXGISwapChain*>(chain)->GetDesc(&description))) {
        return nullptr;
    }
    return description.OutputWindow;
}

uint32_t same_object(void* first, void* second)
{
    if (!first || !second) {
        return 0;
    }
    IUnknown* a = nullptr;
    IUnknown* b = nullptr;
    const bool queried =
        SUCCEEDED(static_cast<IUnknown*>(first)->QueryInterface(__uuidof(IUnknown),
                                                               reinterpret_cast<void**>(&a))) &&
        SUCCEEDED(static_cast<IUnknown*>(second)->QueryInterface(__uuidof(IUnknown),
                                                                reinterpret_cast<void**>(&b)));
    const bool same = queried && a && a == b;
    if (a) {
        a->Release();
    }
    if (b) {
        b->Release();
    }
    return same ? 1u : 0u;
}

void* immediate_context(void* device)
{
    ID3D11DeviceContext* context = nullptr;
    static_cast<ID3D11Device*>(device)->GetImmediateContext(&context);
    return context;
}

void* texture_device(void* texture)
{
    ID3D11Device* device = nullptr;
    static_cast<ID3D11Texture2D*>(texture)->GetDevice(&device);
    return device;
}

void texture_desc(void* texture, rsf_overlay_host_texture_desc* out)
{
    D3D11_TEXTURE2D_DESC description{};
    static_cast<ID3D11Texture2D*>(texture)->GetDesc(&description);
    *out = {description.Width, description.Height, static_cast<uint32_t>(description.Format),
            description.BindFlags};
}

int32_t create_view(void* device, void* texture, void** view)
{
    return static_cast<ID3D11Device*>(device)->CreateRenderTargetView(
        static_cast<ID3D11Texture2D*>(texture), nullptr,
        reinterpret_cast<ID3D11RenderTargetView**>(view));
}

/* OMSetRenderTargets also unbinds every unordered access view the output merger holds and those
   cannot be put back exactly. At Present the game's last draw has already happened, which is the
   same trade `present_blit` makes at the same point in the frame. */
void save_targets(void* context, rsf_overlay_host_targets* out)
{
    static_cast<ID3D11DeviceContext*>(context)->OMGetRenderTargets(
        D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT,
        reinterpret_cast<ID3D11RenderTargetView**>(out->views),
        reinterpret_cast<ID3D11DepthStencilView**>(&out->depth));
}

void bind_target(void* context, void* view)
{
    auto* target = static_cast<ID3D11RenderTargetView*>(view);
    static_cast<ID3D11DeviceContext*>(context)->OMSetRenderTargets(1, &target, nullptr);
}

void restore_targets(void* context, const rsf_overlay_host_targets* saved)
{
    static_cast<ID3D11DeviceContext*>(context)->OMSetRenderTargets(
        D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT,
        reinterpret_cast<ID3D11RenderTargetView* const*>(saved->views),
        static_cast<ID3D11DepthStencilView*>(saved->depth));
}

int32_t renderer_create(void* device, rsf_overlay_host_log log, void* user, void** out)
{
    rsf_overlay_renderer_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_OVERLAY_RENDERER_ABI_VERSION;
    setup.log = log;
    setup.log_user = user;
    return rsf_overlay_renderer_create(device, &setup,
                                       reinterpret_cast<rsf_overlay_renderer**>(out));
}

int32_t renderer_upload(void* renderer, void* context, const rsf_overlay_texture_update* update)
{
    return rsf_overlay_renderer_upload_texture(static_cast<rsf_overlay_renderer*>(renderer),
                                               context, update);
}

void renderer_free(void* renderer, uint64_t id)
{
    rsf_overlay_renderer_free_texture(static_cast<rsf_overlay_renderer*>(renderer), id);
}

int32_t renderer_draw(void* renderer, void* context, const rsf_overlay_draw_data* data,
                      uint32_t width, uint32_t height)
{
    return rsf_overlay_renderer_draw(static_cast<rsf_overlay_renderer*>(renderer), context, data,
                                     width, height);
}

void renderer_destroy(void* renderer)
{
    rsf_overlay_renderer_destroy(static_cast<rsf_overlay_renderer*>(renderer));
}

int32_t input_install(void* window, uint32_t toggle_key, rsf_overlay_host_log log, void* user)
{
    rsf_overlay_input_options options{};
    options.struct_size = sizeof(options);
    options.abi_version = RSF_OVERLAY_INPUT_ABI_VERSION;
    options.toggle_virtual_key = toggle_key;
    options.log = log;
    options.log_user = user;
    return rsf_overlay_input_install(window, &options);
}

void input_uninstall()
{
    rsf_overlay_input_uninstall();
}

void input_collect(rsf_overlay_input* out, uint32_t width, uint32_t height, float delta_seconds)
{
    rsf_overlay_input_collect(out, width, height, delta_seconds);
}

uint64_t tick()
{
    return GetTickCount64();
}

uint32_t counter(int64_t* value)
{
    LARGE_INTEGER now{};
    if (!QueryPerformanceCounter(&now)) {
        return 0;
    }
    *value = now.QuadPart;
    return 1;
}

uint32_t frequency(int64_t* value)
{
    LARGE_INTEGER rate{};
    if (!QueryPerformanceFrequency(&rate)) {
        return 0;
    }
    *value = rate.QuadPart;
    return 1;
}

uint32_t thread_id()
{
    return GetCurrentThreadId();
}

constexpr rsf_overlay_host_platform platform{
    .load_panel = load_panel,
    .chain_device = chain_device,
    .chain_buffer = chain_buffer,
    .chain_window = chain_window,
    .same_object = same_object,
    .immediate_context = immediate_context,
    .texture_device = texture_device,
    .texture_desc = texture_desc,
    .create_view = create_view,
    .save_targets = save_targets,
    .bind_target = bind_target,
    .restore_targets = restore_targets,
    .renderer_create = renderer_create,
    .renderer_upload = renderer_upload,
    .renderer_free = renderer_free,
    .renderer_draw = renderer_draw,
    .renderer_destroy = renderer_destroy,
    .input_install = input_install,
    .input_uninstall = input_uninstall,
    .input_visible = rsf_overlay_input_visible,
    .input_set_visible = rsf_overlay_input_set_visible,
    .input_collect = input_collect,
    .tick = tick,
    .counter = counter,
    .frequency = frequency,
    .thread_id = thread_id,
};

} // namespace

extern "C" int rsf_overlay_host_start(void* swapchain, rsf_overlay_host_log_fn log, void* log_user)
{
    return rsf_core_overlay_host_start(&platform, swapchain, log, log_user);
}

extern "C" int rsf_overlay_host_start_device(void* native_device, void* hwnd,
                                             rsf_overlay_host_log_fn logger, void* user)
{
    return rsf_core_overlay_host_start_device(&platform, native_device, hwnd, logger, user);
}

extern "C" int rsf_overlay_host_draw_target(void* native_context, void* native_target,
                                            uint32_t width, uint32_t height,
                                            const rsf_overlay_stats* stats,
                                            rsf_overlay_intent* intent)
{
    return rsf_core_overlay_host_draw_target(&platform, native_context, native_target, width,
                                             height, stats, intent);
}

extern "C" unsigned int rsf_overlay_host_visible(void)
{
    return rsf_core_overlay_host_visible(&platform);
}

extern "C" void rsf_overlay_host_toggle(void)
{
    rsf_core_overlay_host_toggle(&platform);
}

extern "C" int rsf_overlay_host_present(void* swapchain, const rsf_overlay_stats* stats,
                                        rsf_overlay_intent* intent)
{
    return rsf_core_overlay_host_present(&platform, swapchain, stats, intent);
}

extern "C" void rsf_overlay_host_stop(void)
{
    rsf_core_overlay_host_stop(&platform);
}

extern "C" void rsf_overlay_host_stop_device(void* native_device)
{
    rsf_core_overlay_host_stop_device(&platform, native_device);
}
