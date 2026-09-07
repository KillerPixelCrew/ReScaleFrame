/* SPDX-License-Identifier: GPL-3.0-only */
/* The caller the overlay never had. See overlay_host.h for why it lives here.

   C++ rather than C, unlike the rest of the proxy, because this is the one part of the bridge that
   has to talk to COM directly: the swap chain is asked for the window it was created against and
   for its back buffer, and neither is worth a C vtable dance when the graphics modules next door
   are already C++. */

#include <windows.h>

#include <d3d11.h>
#include <dxgi.h>

#include <rescaleframe/overlay.h>
#include <rescaleframe/overlay_input.h>
#include <rescaleframe/overlay_renderer.h>

#include <atomic>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <cwchar>

#include "overlay_host.h"

namespace {

/* The key that opens the panel. F6 through F11 are already claimed by the bridge, and the input
   module's own default is F7, which is the debug display. */
constexpr unsigned int kToggleKey = VK_F5;

/* Atlas changes are drained in batches rather than one at a time. egui sends the font atlas whole
   once and then a patch per glyph first used, so a frame that introduces a word can produce
   several at once and none of them can be skipped. */
constexpr unsigned int kUpdateBatch = 8;

using create_fn = rsf_overlay* (*)(uint32_t);
using destroy_fn = void (*)(rsf_overlay*);
using frame_fn = rsf_overlay_result (*)(rsf_overlay*, const rsf_overlay_input*,
                                        const rsf_overlay_stats*, rsf_overlay_draw_data*,
                                        rsf_overlay_intent*);
using texture_updates_fn = uint32_t (*)(rsf_overlay*, rsf_overlay_texture_update*, uint32_t);
using textures_to_free_fn = uint32_t (*)(rsf_overlay*, uint64_t*, uint32_t);

struct Host {
    HMODULE panel_module = nullptr;
    create_fn create = nullptr;
    destroy_fn destroy = nullptr;
    frame_fn frame = nullptr;
    texture_updates_fn texture_updates = nullptr;
    textures_to_free_fn textures_to_free = nullptr;

    rsf_overlay* panel = nullptr;
    rsf_overlay_renderer* renderer = nullptr;
    ID3D11Device* device = nullptr;

    bool started = false;
    /* Set when a frame failed in a way that will fail again every frame. The panel is closed and
       left closed rather than reporting the same line sixty times a second. */
    bool stopped_after_failure = false;

    /* How many drawn frames still announce each step before taking it.

       The first frame of this path does several things no test reaches: it lays out egui for the
       first time, uploads a whole font atlas, and issues draws into a game's frame. A crash in any
       of them looks identical from outside, and note() writes and closes per line, so the last line
       in the log is the step that did not survive. Only the first few frames, because after that
       the same lines would bury the run. */
    unsigned int trace_frames = 4;

    /* Held while a frame is being drawn.

       A game may present from more than one thread, and this path creates D3D11 resources and
       issues draws against one device. Two of them at once is not something the panel should ever
       do, whatever the device's threading mode allows. The second thread skips its frame rather
       than waiting, because waiting on a render thread to draw a diagnostic is a worse trade than
       missing one frame of it. */
    std::atomic<bool> drawing{false};

    LARGE_INTEGER frequency{};
    LARGE_INTEGER last_frame{};

    rsf_overlay_host_log_fn log = nullptr;
    void* log_user = nullptr;
};

Host& host()
{
    static Host instance;
    return instance;
}

void say(const char* format, ...)
{
    Host& self = host();
    if (!self.log) {
        return;
    }
    char message[512];
    va_list arguments;
    va_start(arguments, format);
    const int written = std::vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    if (written < 0) {
        return;
    }
    self.log(self.log_user, message);
}

void log_from_module(void* user, const char* message)
{
    (void)user;
    say("%s", message);
}

/* Where the panel DLL is. An explicit path wins, because a research build is often assembled by
   hand; otherwise it is looked for beside this module, which is where a packaged one would sit. */
bool panel_path(wchar_t* out, size_t count)
{
    const DWORD explicit_length = GetEnvironmentVariableW(L"RSF_OVERLAY_DLL", out,
                                                          static_cast<DWORD>(count));
    if (explicit_length > 0 && explicit_length < count) {
        return true;
    }

    HMODULE self = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                            reinterpret_cast<LPCWSTR>(&panel_path), &self)) {
        return false;
    }
    const DWORD length = GetModuleFileNameW(self, out, static_cast<DWORD>(count));
    if (length == 0 || length >= count) {
        return false;
    }
    wchar_t* separator = std::wcsrchr(out, L'\\');
    if (!separator) {
        return false;
    }
    separator[1] = L'\0';
    if (std::wcslen(out) + std::wcslen(L"rescaleframe_overlay.dll") >= count) {
        return false;
    }
    std::wcscat(out, L"rescaleframe_overlay.dll");
    return true;
}

bool load_panel()
{
    Host& self = host();
    wchar_t path[MAX_PATH * 2];
    if (!panel_path(path, sizeof(path) / sizeof(path[0]))) {
        say("overlay: cannot work out where the panel DLL is");
        return false;
    }

    self.panel_module = LoadLibraryW(path);
    if (!self.panel_module) {
        say("overlay: the panel DLL did not load, error %lu. Set RSF_OVERLAY_DLL to point at "
            "rescaleframe_overlay.dll",
            GetLastError());
        return false;
    }

    self.create = reinterpret_cast<create_fn>(
        reinterpret_cast<void*>(GetProcAddress(self.panel_module, "rsf_overlay_create")));
    self.destroy = reinterpret_cast<destroy_fn>(
        reinterpret_cast<void*>(GetProcAddress(self.panel_module, "rsf_overlay_destroy")));
    self.frame = reinterpret_cast<frame_fn>(
        reinterpret_cast<void*>(GetProcAddress(self.panel_module, "rsf_overlay_frame")));
    self.texture_updates = reinterpret_cast<texture_updates_fn>(
        reinterpret_cast<void*>(GetProcAddress(self.panel_module, "rsf_overlay_texture_updates")));
    self.textures_to_free = reinterpret_cast<textures_to_free_fn>(
        reinterpret_cast<void*>(GetProcAddress(self.panel_module, "rsf_overlay_textures_to_free")));

    if (!self.create || !self.destroy || !self.frame || !self.texture_updates ||
        !self.textures_to_free) {
        say("overlay: the panel DLL loaded but does not export the five entry points");
        FreeLibrary(self.panel_module);
        self.panel_module = nullptr;
        return false;
    }
    return true;
}

/* The window the swap chain presents to. Asked for rather than searched for: a game process has
   several windows and only the swap chain knows which one its frames land in. */
HWND window_of(IDXGISwapChain* chain)
{
    DXGI_SWAP_CHAIN_DESC description{};
    if (FAILED(chain->GetDesc(&description))) {
        return nullptr;
    }
    return description.OutputWindow;
}

float seconds_since_last_frame()
{
    Host& self = host();
    LARGE_INTEGER now{};
    if (self.frequency.QuadPart == 0 || !QueryPerformanceCounter(&now)) {
        return 1.0f / 60.0f;
    }
    if (self.last_frame.QuadPart == 0) {
        self.last_frame = now;
        return 1.0f / 60.0f;
    }
    const double elapsed = double(now.QuadPart - self.last_frame.QuadPart) /
                           double(self.frequency.QuadPart);
    self.last_frame = now;
    /* A frame that took a second is a mission load or a breakpoint, and handing egui that as a
       delta makes every animation jump. */
    if (elapsed <= 0.0 || elapsed > 0.25) {
        return 1.0f / 60.0f;
    }
    return float(elapsed);
}

/* Hand the renderer everything the last layout changed, then everything it has finished with. Both
   drain until they come back empty, because each call returns only what fits in the array. */
void carry_textures(ID3D11DeviceContext* context)
{
    Host& self = host();

    rsf_overlay_texture_update updates[kUpdateBatch];
    for (;;) {
        const uint32_t count = self.texture_updates(self.panel, updates, kUpdateBatch);
        for (uint32_t index = 0; index < count; ++index) {
            if (rsf_overlay_renderer_upload_texture(self.renderer, context, &updates[index]) !=
                RSF_OVERLAY_RENDERER_OK) {
                say("overlay: an atlas update was refused, so some text will be missing");
            }
        }
        if (count < kUpdateBatch) {
            break;
        }
    }

    uint64_t finished[kUpdateBatch];
    for (;;) {
        const uint32_t count = self.textures_to_free(self.panel, finished, kUpdateBatch);
        for (uint32_t index = 0; index < count; ++index) {
            rsf_overlay_renderer_free_texture(self.renderer, finished[index]);
        }
        if (count < kUpdateBatch) {
            break;
        }
    }
}

// These references exist only inside a Present callback, including on early returns.
// In particular, no view or texture is retained across Present or ResizeBuffers.
template <typename T> struct LocalRef {
    T* value = nullptr;
    LocalRef() = default;
    LocalRef(const LocalRef&) = delete;
    LocalRef& operator=(const LocalRef&) = delete;
    ~LocalRef()
    {
        if (value) {
            value->Release();
        }
    }
};

bool same_device(ID3D11Device* a, ID3D11Device* b)
{
    if (!a || !b) {
        return false;
    }
    LocalRef<IUnknown> first;
    LocalRef<IUnknown> second;
    return SUCCEEDED(a->QueryInterface(__uuidof(IUnknown),
                                       reinterpret_cast<void**>(&first.value))) &&
           SUCCEEDED(b->QueryInterface(__uuidof(IUnknown),
                                       reinterpret_cast<void**>(&second.value))) &&
           first.value && first.value == second.value;
}

/* Saved around the draw, because binding a target displaces whatever the game had.

   OMSetRenderTargets also unbinds every unordered access view the output merger holds and those
   cannot be put back exactly. At Present the game's last draw has already happened, which is the
   same trade `present_blit` makes at the same point in the frame. */
struct SavedTargets {
    ID3D11RenderTargetView* targets[D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT] = {};
    ID3D11DepthStencilView* depth = nullptr;
};

void save_targets(ID3D11DeviceContext* context, SavedTargets& saved)
{
    context->OMGetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, saved.targets,
                                &saved.depth);
}

void restore_targets(ID3D11DeviceContext* context, SavedTargets& saved)
{
    context->OMSetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, saved.targets, saved.depth);
    for (ID3D11RenderTargetView* view : saved.targets) {
        if (view) {
            view->Release();
        }
    }
    if (saved.depth) {
        saved.depth->Release();
    }
}

} // namespace

extern "C" int rsf_overlay_host_start(void* swapchain, rsf_overlay_host_log_fn log,
                                      void* log_user)
{
    Host& self = host();
    self.log = log;
    self.log_user = log_user;

    if (self.started) {
        return 1;
    }
    if (self.stopped_after_failure) {
        return 0;
    }
    if (!swapchain) {
        return 0;
    }

    auto* chain = static_cast<IDXGISwapChain*>(swapchain);
    LocalRef<ID3D11Device> device;
    const HRESULT got_device = chain->GetDevice(__uuidof(ID3D11Device),
                                                reinterpret_cast<void**>(&device.value));
    if (FAILED(got_device) || !device.value) {
        say("overlay: presenting swap chain has no D3D11 device, hr 0x%08lx",
            (unsigned long)got_device);
        return 0;
    }
    say("overlay: swap chain %p selects device %p", swapchain, (void*)device.value);

    if (!self.panel_module && !load_panel()) {
        self.stopped_after_failure = true;
        return 0;
    }

    self.panel = self.create(RSF_OVERLAY_ABI_VERSION);
    if (!self.panel) {
        say("overlay: the panel refused to be created, which is an ABI mismatch or an allocation "
            "failure");
        self.stopped_after_failure = true;
        return 0;
    }

    rsf_overlay_renderer_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_OVERLAY_RENDERER_ABI_VERSION;
    setup.log = log_from_module;
    setup.log_user = nullptr;
    if (rsf_overlay_renderer_create(device.value, &setup, &self.renderer) != RSF_OVERLAY_RENDERER_OK) {
        say("overlay: the renderer would not build, so there is nothing to draw with");
        self.destroy(self.panel);
        self.panel = nullptr;
        self.stopped_after_failure = true;
        return 0;
    }

    HWND window = window_of(chain);
    if (!window) {
        say("overlay: the swap chain would not say which window it presents to");
        rsf_overlay_renderer_destroy(self.renderer);
        self.renderer = nullptr;
        self.destroy(self.panel);
        self.panel = nullptr;
        self.stopped_after_failure = true;
        return 0;
    }

    rsf_overlay_input_options options{};
    options.struct_size = sizeof(options);
    options.abi_version = RSF_OVERLAY_INPUT_ABI_VERSION;
    options.toggle_virtual_key = kToggleKey;
    options.log = log_from_module;
    options.log_user = nullptr;
    const rsf_overlay_input_result installed = rsf_overlay_input_install(window, &options);
    if (installed != RSF_OVERLAY_INPUT_OK) {
        say("overlay: the window would not take the input hook, result %d. The panel would draw "
            "and not answer a mouse, so it is not started",
            int(installed));
        rsf_overlay_renderer_destroy(self.renderer);
        self.renderer = nullptr;
        self.destroy(self.panel);
        self.panel = nullptr;
        self.stopped_after_failure = true;
        return 0;
    }

    self.device = device.value;
    self.device->AddRef();
    QueryPerformanceFrequency(&self.frequency);
    self.last_frame.QuadPart = 0;
    self.started = true;

    say("overlay: ready. F5 opens it, and it draws over the finished frame without taking part in "
        "the reconstruction");
    return 1;
}

extern "C" unsigned int rsf_overlay_host_visible(void)
{
    if (!host().started) {
        return 0;
    }
    return rsf_overlay_input_visible();
}

extern "C" void rsf_overlay_host_toggle(void)
{
    if (!host().started) {
        say("overlay: not running, so there is nothing to open");
        return;
    }
    rsf_overlay_input_set_visible(rsf_overlay_input_visible() ? 0u : 1u);
}

extern "C" int rsf_overlay_host_present(void* swapchain,
                                        const rsf_overlay_stats* stats, rsf_overlay_intent* intent)
{
    Host& self = host();
    if (!self.started || self.stopped_after_failure || !swapchain || !stats) {
        return 0;
    }
    if (!rsf_overlay_input_visible()) {
        /* Closed. The clock is reset so that reopening it does not hand egui the whole time the
           panel spent shut as one frame. */
        self.last_frame.QuadPart = 0;
        return 0;
    }

    bool idle = false;
    if (!self.drawing.compare_exchange_strong(idle, true)) {
        say("overlay frame: thread %lu skipped, another is already drawing the panel",
            (unsigned long)GetCurrentThreadId());
        return 0;
    }
    // Cleared on every path out, including the early returns below.
    struct DrawingGuard {
        Host& host;
        ~DrawingGuard() { host.drawing.store(false); }
    } drawing_guard{self};

    const bool trace = self.trace_frames > 0;
    if (trace) {
        say("overlay frame: visible on thread %lu, acquiring the back buffer",
            (unsigned long)GetCurrentThreadId());
    }

    auto* chain = static_cast<IDXGISwapChain*>(swapchain);
    LocalRef<ID3D11Device> chain_device;
    if (FAILED(chain->GetDevice(__uuidof(ID3D11Device),
                                reinterpret_cast<void**>(&chain_device.value))) ||
        !same_device(self.device, chain_device.value)) {
        // Shared vtables can also dispatch Present for another device. Our renderer belongs to
        // the device selected at start; never submit its resources through a different context.
        if (trace) {
            say("overlay: skipping swap chain %p on another device %p (renderer %p)", swapchain,
                (void*)chain_device.value, (void*)self.device);
            --self.trace_frames;
        }
        return 0;
    }
    LocalRef<ID3D11DeviceContext> immediate;
    chain_device.value->GetImmediateContext(&immediate.value);
    if (!immediate.value) {
        return 0;
    }
    auto* device_context = immediate.value;
    LocalRef<ID3D11Texture2D> back_buffer;
    const HRESULT got_buffer = chain->GetBuffer(0, __uuidof(ID3D11Texture2D),
                                                reinterpret_cast<void**>(&back_buffer.value));
    if (FAILED(got_buffer) || !back_buffer.value) {
        if (trace) {
            say("overlay: GetBuffer failed, hr 0x%08lx", (unsigned long)got_buffer);
            --self.trace_frames;
        }
        return 0;
    }
    LocalRef<ID3D11Device> owner;
    back_buffer.value->GetDevice(&owner.value);
    if (!same_device(chain_device.value, owner.value)) {
        say("overlay: back buffer %p belongs to device %p, chain device %p. Closing panel",
            (void*)back_buffer.value, (void*)owner.value, (void*)chain_device.value);
        self.stopped_after_failure = true;
        rsf_overlay_input_set_visible(0u);
        return 0;
    }
    D3D11_TEXTURE2D_DESC description{};
    back_buffer.value->GetDesc(&description);
    if (description.Width == 0 || description.Height == 0) {
        return 0;
    }
    LocalRef<ID3D11RenderTargetView> target;
    if (trace) {
        say("overlay view: chain %p, device %p, context %p, buffer %p, owner %p, "
            "%ux%u format %u bind 0x%x. Creating view",
            swapchain, (void*)chain_device.value, (void*)device_context, (void*)back_buffer.value,
            (void*)owner.value, description.Width, description.Height,
            (unsigned int)description.Format, description.BindFlags);
    }
    const HRESULT made = chain_device.value->CreateRenderTargetView(back_buffer.value, nullptr,
                                                                     &target.value);
    if (trace) {
        say("overlay view: CreateRenderTargetView returned hr 0x%08lx, view %p",
            (unsigned long)made, (void*)target.value);
    }
    if (FAILED(made) || !target.value) {
        say("overlay: no back-buffer view, hr 0x%08lx. Closing panel", (unsigned long)made);
        self.stopped_after_failure = true;
        rsf_overlay_input_set_visible(0u);
        return 0;
    }

    rsf_overlay_input input{};
    input.struct_size = sizeof(input);
    rsf_overlay_input_collect(&input, description.Width, description.Height,
                              seconds_since_last_frame());

    rsf_overlay_draw_data draw_data{};
    draw_data.struct_size = sizeof(draw_data);
    rsf_overlay_intent decided{};
    decided.struct_size = sizeof(decided);

    if (trace) {
        say("overlay frame: laying out the panel");
    }
    const rsf_overlay_result laid_out = self.frame(self.panel, &input, stats, &draw_data, &decided);
    if (laid_out != RSF_OVERLAY_OK) {
        /* A panicked panel is poisoned for good and every later frame returns the same thing, so
           it is closed here rather than reported once per frame forever. */
        say("overlay: the panel failed to lay out a frame, result %d. Closing it", int(laid_out));
        rsf_overlay_input_set_visible(0u);
        return 0;
    }

    if (trace) {
        say("overlay frame: laid out, %u vertices, %u indices, %u calls. Carrying textures",
            draw_data.vertex_count, draw_data.index_count, draw_data.call_count);
    }
    carry_textures(device_context);

    SavedTargets saved;
    save_targets(device_context, saved);
    // Restore before target is released, on every path out of the draw scope.
    struct TargetGuard {
        ID3D11DeviceContext* context;
        SavedTargets& saved;
        ~TargetGuard() { restore_targets(context, saved); }
    } target_guard{device_context, saved};
    device_context->OMSetRenderTargets(1, &target.value, nullptr);

    if (trace) {
        say("overlay frame: calling the renderer");
    }
    const rsf_overlay_renderer_result drawn = rsf_overlay_renderer_draw(
        self.renderer, device_context, &draw_data, description.Width, description.Height);
    if (trace) {
        say("overlay frame: the renderer returned %d", int(drawn));
    }

    if (trace) {
        say("overlay frame: drawn into the back buffer, result %d", int(drawn));
        --self.trace_frames;
    }

    if (drawn != RSF_OVERLAY_RENDERER_OK) {
        say("overlay: the panel laid out a frame the renderer would not draw, result %d",
            int(drawn));
        return 0;
    }

    if (intent) {
        *intent = decided;
    }
    return 1;
}

extern "C" void rsf_overlay_host_stop(void)
{
    Host& self = host();
    if (!self.started) {
        return;
    }
    self.started = false;
    rsf_overlay_input_set_visible(0u);
    rsf_overlay_input_uninstall();

    if (self.renderer) {
        rsf_overlay_renderer_destroy(self.renderer);
        self.renderer = nullptr;
    }
    if (self.panel && self.destroy) {
        self.destroy(self.panel);
        self.panel = nullptr;
    }
    if (self.device) {
        self.device->Release();
        self.device = nullptr;
    }
    /* The panel DLL is deliberately left loaded. Freeing it would unload Rust code that a window
       message already inside the input hook can still be on its way into. */
}
