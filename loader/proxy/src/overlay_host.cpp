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

    /* The back buffer's render target view, made once and kept.

       Made once because making one per frame is what took the game down, and kept keyed on the
       back buffer it was made from so a swap chain resize rebuilds it rather than binding a view
       onto a texture that no longer exists. Null means we draw into whatever the game left bound,
       which is correct but often invisible. */
    ID3D11Texture2D* target_texture = nullptr;
    ID3D11RenderTargetView* target_view = nullptr;
    /* Set once creating the view has failed, so it is attempted once and not once per frame. */
    bool target_refused = false;

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

/* The back buffer's view, made at most once.

   The device comes from the context we are about to draw with, not from the pointer stored when
   the host started. That is the one difference between this and the version that took the game
   down: a view is only valid when its resource and the device agree, and a device captured at
   startup is a guess about which device that is. `present_blit` makes the same call successfully
   with a device it was handed at the moment it was created.

   Returns null when there is no view to be had, and sets `target_refused` so the attempt is not
   repeated every frame. The caller then draws into whatever the game left bound. */
ID3D11RenderTargetView* back_buffer_view(Host& self, ID3D11DeviceContext* context,
                                         IDXGISwapChain* chain)
{
    if (self.target_refused) {
        return nullptr;
    }

    ID3D11Texture2D* back_buffer = nullptr;
    const HRESULT got_buffer =
        chain->GetBuffer(0, __uuidof(ID3D11Texture2D), reinterpret_cast<void**>(&back_buffer));
    if (FAILED(got_buffer) || !back_buffer) {
        say("overlay: the swap chain would not hand over its back buffer, hr 0x%08lx. The panel "
            "will draw into whatever is bound",
            (unsigned long)got_buffer);
        self.target_refused = true;
        return nullptr;
    }

    // The same texture as last time means the view we already have is still the right one.
    if (self.target_view && self.target_texture == back_buffer) {
        back_buffer->Release();
        return self.target_view;
    }

    if (self.target_view) {
        self.target_view->Release();
        self.target_view = nullptr;
    }
    if (self.target_texture) {
        self.target_texture->Release();
        self.target_texture = nullptr;
    }

    ID3D11Device* device = nullptr;
    context->GetDevice(&device);
    if (!device) {
        back_buffer->Release();
        say("overlay: the context would not name its device, so the panel will draw into whatever "
            "is bound");
        self.target_refused = true;
        return nullptr;
    }

    ID3D11RenderTargetView* view = nullptr;
    const HRESULT made = device->CreateRenderTargetView(back_buffer, nullptr, &view);
    device->Release();
    if (FAILED(made) || !view) {
        back_buffer->Release();
        say("overlay: no render target view over the back buffer, hr 0x%08lx. The panel will draw "
            "into whatever is bound",
            (unsigned long)made);
        self.target_refused = true;
        return nullptr;
    }

    self.target_texture = back_buffer;
    self.target_view = view;
    say("overlay: drawing into the back buffer through a view made once, kept while it lasts");
    return view;
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

extern "C" int rsf_overlay_host_start(void* device, void* swapchain, rsf_overlay_host_log_fn log,
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
    if (!device || !swapchain) {
        return 0;
    }

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
    if (rsf_overlay_renderer_create(device, &setup, &self.renderer) != RSF_OVERLAY_RENDERER_OK) {
        say("overlay: the renderer would not build, so there is nothing to draw with");
        self.destroy(self.panel);
        self.panel = nullptr;
        self.stopped_after_failure = true;
        return 0;
    }

    auto* chain = static_cast<IDXGISwapChain*>(swapchain);
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

    self.device = static_cast<ID3D11Device*>(device);
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

extern "C" int rsf_overlay_host_present(void* context, void* swapchain,
                                        const rsf_overlay_stats* stats, rsf_overlay_intent* intent)
{
    Host& self = host();
    if (!self.started || !context || !swapchain || !stats) {
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
        // The thread id, because two of these sequences reached CreateRenderTargetView and neither
        // returned, which one thread cannot do. If the ids differ, the game presents from more than
        // one thread and this whole path is being run concurrently against one device.
        say("overlay frame: visible on thread %lu, acquiring the back buffer",
            (unsigned long)GetCurrentThreadId());
    }

    auto* device_context = static_cast<ID3D11DeviceContext*>(context);
    auto* chain = static_cast<IDXGISwapChain*>(swapchain);

    /* The size comes from the swap chain's own description rather than from its back buffer.

       Taking the back buffer and making a render target view over it is what took the game down:
       the log reached the line before `CreateRenderTargetView` and never the one after, twice, on
       one thread. It is also work this does not need. `overlay_renderer` draws into whatever is
       bound when it is called and deliberately never rebinds, because `OMSetRenderTargets` unbinds
       every unordered access view the output merger holds and those cannot be put back exactly.
       Present is after the game's last draw, so what is bound is the image about to be shown.

       The consequence, and it is a real one: if the game leaves nothing bound at Present, the
       panel draws nowhere and is simply not visible. That is a diagnostic worth having over a
       process that dies. */
    DXGI_SWAP_CHAIN_DESC chain_description{};
    const HRESULT got_desc = chain->GetDesc(&chain_description);
    if (FAILED(got_desc)) {
        say("overlay frame: the swap chain would not describe itself, hr 0x%08lx",
            (unsigned long)got_desc);
        return 0;
    }
    D3D11_TEXTURE2D_DESC description{};
    description.Width = chain_description.BufferDesc.Width;
    description.Height = chain_description.BufferDesc.Height;
    if (description.Width == 0 || description.Height == 0) {
        return 0;
    }
    if (trace) {
        say("overlay frame: presenting at %ux%u, drawing into whatever is bound",
            description.Width, description.Height);
    }

    if (trace) {
        say("overlay frame: target %ux%u, collecting input", description.Width,
            description.Height);
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

    if (trace) {
        /* What the game left bound. Read only, and the answer decides whether the panel is drawing
           into nothing or into a target that is not the presented image. */
        ID3D11RenderTargetView* bound[D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT] = {};
        ID3D11DepthStencilView* bound_depth = nullptr;
        device_context->OMGetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, bound,
                                           &bound_depth);
        unsigned int bound_count = 0;
        for (ID3D11RenderTargetView* view : bound) {
            if (view) {
                ++bound_count;
            }
        }
        if (bound_count > 0 && bound[0]) {
            ID3D11Resource* resource = nullptr;
            bound[0]->GetResource(&resource);
            D3D11_RENDER_TARGET_VIEW_DESC view_description{};
            bound[0]->GetDesc(&view_description);
            say("overlay frame: %u render targets bound, first is resource %p format %d, depth %s",
                bound_count, (void*)resource, (int)view_description.Format,
                bound_depth ? "yes" : "no");
            if (resource) {
                resource->Release();
            }
        } else {
            say("overlay frame: nothing is bound at present, so the panel draws nowhere");
        }
        for (ID3D11RenderTargetView* view : bound) {
            if (view) {
                view->Release();
            }
        }
        if (bound_depth) {
            bound_depth->Release();
        }
        say("overlay frame: textures carried, drawing");
    }

    /* Bind the back buffer if we can have a view onto it, and put back what was bound afterwards.
       Without one the renderer draws into whatever the game left, which is correct and frequently
       invisible. */
    ID3D11RenderTargetView* view = back_buffer_view(self, device_context, chain);
    SavedTargets saved;
    if (view) {
        save_targets(device_context, saved);
        device_context->OMSetRenderTargets(1, &view, nullptr);
    }

    const rsf_overlay_renderer_result drawn = rsf_overlay_renderer_draw(
        self.renderer, device_context, &draw_data, description.Width, description.Height);

    if (view) {
        restore_targets(device_context, saved);
    }

    if (trace) {
        say("overlay frame: drawn into %s, result %d", view ? "the back buffer" : "what was bound",
            int(drawn));
    }
    if (trace) {
        say("overlay frame: complete");
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

    if (self.target_view) {
        self.target_view->Release();
        self.target_view = nullptr;
    }
    if (self.target_texture) {
        self.target_texture->Release();
        self.target_texture = nullptr;
    }
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
