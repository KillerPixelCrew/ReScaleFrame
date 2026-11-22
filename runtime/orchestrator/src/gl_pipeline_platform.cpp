// SPDX-License-Identifier: MIT
// Win32, DXGI, D3D12, GL sharing, SR, FG and presentation-owner calls for the Rust OpenGL pipeline
// owner in gl_pipeline.rs. Each leaf performs one platform operation. Session lifecycle, frame
// identity, resource and cache state, admission, backend switching and fallback are decided in
// Rust. Returned COM interfaces carry one reference for the caller.
#include <rescaleframe/gl_pipeline.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/streamline_host.h>
#include <rescaleframe/d3d11_present_bridge.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
using Microsoft::WRL::ComPtr;

// Values gl_pipeline.rs names directly.
static_assert(RSF_SR_NONE == 0 && RSF_SR_DLSS == 1 && RSF_SR_FSR2 == 2 && RSF_SR_FSR3 == 3 && RSF_SR_FSR4 == 4 &&
              RSF_SR_XESS == 5 && RSF_SR_SPATIAL == 6 && RSF_SR_AUTO == 7);
static_assert(RSF_DLSS_ENGINE_CUSTOM == 0);
static_assert(D3D12_RESOURCE_STATE_COMMON == 0 && D3D12_RESOURCE_STATE_PRESENT == 0 &&
              D3D12_RESOURCE_STATE_RENDER_TARGET == 0x4 && D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE == 0x40 &&
              D3D12_RESOURCE_STATE_COPY_DEST == 0x400 && D3D12_RESOURCE_STATE_COPY_SOURCE == 0x800);
static_assert(DXGI_FORMAT_R8G8B8A8_UNORM == 28);

namespace {
constexpr UINT surface_resize = WM_APP + 0x525, surface_destroy = WM_APP + 0x526;

LRESULT CALLBACK surface_window(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    // GLFW remains the input/focus owner. This child owns only DXGI presentation.
    if (message == WM_NCHITTEST) return HTTRANSPARENT;
    if (message == WM_MOUSEACTIVATE) return MA_NOACTIVATE;
    if (message == WM_ERASEBKGND) return 1;
    if (message == surface_resize)
        return SetWindowPos(window, nullptr, 0, 0, static_cast<int>(wparam), static_cast<int>(lparam),
                            SWP_NOACTIVATE | SWP_NOZORDER);
    if (message == surface_destroy) return DestroyWindow(window);
    return DefWindowProcW(window, message, wparam, lparam);
}

HWND create_surface(HWND parent)
{
    if (GetWindowThreadProcessId(parent, nullptr) != GetCurrentThreadId()) return nullptr;
    WNDCLASSW type{};
    type.lpfnWndProc = surface_window;
    type.hInstance = GetModuleHandleW(nullptr);
    type.lpszClassName = L"ReScaleFrame.OpenGL.Presentation";
    if (!RegisterClassW(&type) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) return nullptr;
    return CreateWindowExW(WS_EX_NOACTIVATE | WS_EX_TRANSPARENT, type.lpszClassName, L"", WS_CHILD | WS_CLIPSIBLINGS,
                           0, 0, 32, 32, parent, nullptr, type.hInstance, nullptr);
}

// Routed to the window's event thread; that thread owns creation, size and destruction.
bool surface_message(HWND window, UINT operation, WPARAM wparam = 0, LPARAM lparam = 0)
{
    DWORD_PTR result = 0;
    return SendMessageTimeoutW(window, operation, wparam, lparam, SMTO_BLOCK | SMTO_ABORTIFHUNG, 5000, &result) &&
           result != 0;
}

int window_valid(void* window) { return IsWindow(static_cast<HWND>(window)) ? 1 : 0; }
void* window_create(void* parent) { return create_surface(static_cast<HWND>(parent)); }
int window_destroy(void* window)
{
    const auto hwnd = static_cast<HWND>(window);
    return !IsWindow(hwnd) || surface_message(hwnd, surface_destroy);
}
int window_child_of(void* window, void* parent)
{
    const auto child = static_cast<HWND>(window), owner = static_cast<HWND>(parent);
    return GetParent(child) == owner &&
           GetWindowThreadProcessId(child, nullptr) == GetWindowThreadProcessId(owner, nullptr);
}
int window_resize(void* window, uint32_t width, uint32_t height)
{
    return surface_message(static_cast<HWND>(window), surface_resize, width, height) ? 1 : 0;
}
void window_show(void* window) { ShowWindowAsync(static_cast<HWND>(window), SW_SHOWNOACTIVATE); }

int32_t chain_create(void* queue, void* window, uint32_t buffers, uint32_t format, void** out)
{
    *out = nullptr;
    ComPtr<IDXGIFactory2> factory;
    ComPtr<IDXGISwapChain1> chain;
    ComPtr<IDXGISwapChain3> chain3;
    DXGI_SWAP_CHAIN_DESC1 desc{};
    desc.Width = desc.Height = 32;
    desc.Format = static_cast<DXGI_FORMAT>(format);
    desc.SampleDesc.Count = 1;
    desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.BufferCount = buffers;
    desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    desc.AlphaMode = DXGI_ALPHA_MODE_IGNORE;
    auto result = CreateDXGIFactory1(IID_PPV_ARGS(&factory));
    if (SUCCEEDED(result))
        result = factory->CreateSwapChainForHwnd(static_cast<ID3D12CommandQueue*>(queue), static_cast<HWND>(window),
                                                 &desc, nullptr, nullptr, &chain);
    if (SUCCEEDED(result)) result = chain.As(&chain3);
    if (SUCCEEDED(result)) *out = chain3.Detach();
    return result;
}
void chain_associate(void* chain, void* window)
{
    ComPtr<IDXGIFactory2> factory;
    if (SUCCEEDED(static_cast<IDXGISwapChain3*>(chain)->GetParent(IID_PPV_ARGS(&factory))))
        factory->MakeWindowAssociation(static_cast<HWND>(window), DXGI_MWA_NO_ALT_ENTER);
}
int32_t chain_resize(void* chain, uint32_t buffers, uint32_t width, uint32_t height, uint32_t format)
{
    return static_cast<IDXGISwapChain3*>(chain)->ResizeBuffers(buffers, width, height, static_cast<DXGI_FORMAT>(format), 0);
}
void* chain_back_buffer(void* chain)
{
    const auto swapchain = static_cast<IDXGISwapChain3*>(chain);
    ID3D12Resource* buffer = nullptr;
    if (FAILED(swapchain->GetBuffer(swapchain->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&buffer)))) return nullptr;
    return buffer;
}
int32_t chain_present(void* chain, uint32_t interval) { return static_cast<IDXGISwapChain3*>(chain)->Present(interval, 0); }

int adapter_identity(void* device, uint32_t* vendor, uint32_t* software)
{
    ComPtr<IDXGIFactory4> factory;
    ComPtr<IDXGIAdapter1> adapter;
    DXGI_ADAPTER_DESC1 description{};
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) ||
        FAILED(factory->EnumAdapterByLuid(static_cast<ID3D12Device*>(device)->GetAdapterLuid(), IID_PPV_ARGS(&adapter))) ||
        FAILED(adapter->GetDesc1(&description)))
        return 0;
    *vendor = description.VendorId;
    *software = (description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0;
    return 1;
}

void transition(void* list, void* resource, uint32_t from, uint32_t to)
{
    D3D12_RESOURCE_BARRIER change{};
    change.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    change.Transition = {static_cast<ID3D12Resource*>(resource), D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                         static_cast<D3D12_RESOURCE_STATES>(from), static_cast<D3D12_RESOURCE_STATES>(to)};
    static_cast<ID3D12GraphicsCommandList*>(list)->ResourceBarrier(1, &change);
}
void copy_resource(void* list, void* destination, void* source)
{
    static_cast<ID3D12GraphicsCommandList*>(list)->CopyResource(static_cast<ID3D12Resource*>(destination),
                                                                 static_cast<ID3D12Resource*>(source));
}

rsf_backend_result gl_open(rsf_backend_log_fn log, void* user, rsf_gl_context** out)
{
    rsf_gl_setup sharing{sizeof(sharing), 1, nullptr, nullptr, log, user};
    return rsf_gl_open(&sharing, out);
}

int latency_completed(uint64_t frame)
{
    rsf_native_latency_frame timing{sizeof(timing), 1, frame};
    return rsf_native_latency_frame_get(&timing) && timing.completed;
}
uint32_t pcl_message()
{
    rsf_streamline_latency_status status{};
    status.struct_size = sizeof(status);
    return rsf_streamline_host_latency_status(rsf_d3d11_present_host(), &status) == RSF_BACKEND_OK ? status.pcl_message_id : 0;
}
void* present_host() { return rsf_d3d11_present_host(); }

// Header-inline contract predicates, kept single-sourced in the published headers.
int dlss_preset_valid(uint32_t preset) { return rsf_dlss_preset_valid(preset); }
uint32_t fg_default_backend(uint32_t vendor, uint32_t software) { return rsf_fg_default_backend(vendor, software); }
uint32_t depth_domains_requested(const rsf_game_render_pass* pass) { return rsf_game_depth_domains_requested(&pass->depth_domains); }

// Field order matches `Platform` in gl_pipeline.rs.
struct Platform {
    decltype(&gl_open) gl_open;
    decltype(&rsf_gl_graphics) gl_graphics;
    decltype(&rsf_gl_texture_create) gl_texture_create;
    decltype(&rsf_gl_texture_get) gl_texture_get;
    decltype(&rsf_gl_texture_destroy) gl_texture_destroy;
    decltype(&rsf_gl_begin) gl_begin;
    decltype(&rsf_gl_submit) gl_submit;
    decltype(&rsf_gl_drain) gl_drain;
    decltype(&rsf_gl_close) gl_close;
    decltype(&window_valid) window_valid;
    decltype(&window_create) window_create;
    decltype(&window_destroy) window_destroy;
    decltype(&window_child_of) window_child_of;
    decltype(&window_resize) window_resize;
    decltype(&window_show) window_show;
    decltype(&chain_create) chain_create;
    decltype(&chain_associate) chain_associate;
    decltype(&chain_resize) chain_resize;
    decltype(&chain_back_buffer) chain_back_buffer;
    decltype(&chain_present) chain_present;
    decltype(&adapter_identity) adapter_identity;
    decltype(&transition) transition;
    decltype(&copy_resource) copy_resource;
    decltype(&rsf_sr12_create) sr_create;
    decltype(&rsf_sr12_plan) sr_plan;
    decltype(&rsf_sr12_evaluate) sr_evaluate;
    decltype(&rsf_sr12_set_sharpening) sr_set_sharpening;
    decltype(&rsf_sr12_destroy) sr_destroy;
    decltype(&rsf_fg12_install) fg_install;
    decltype(&rsf_fg12_options) fg_options;
    decltype(&rsf_fg12_cpu) fg_cpu;
    decltype(&rsf_fg12_hudless) fg_hudless;
    decltype(&rsf_fg12_submitted) fg_submitted;
    decltype(&rsf_fg12_window) fg_window;
    decltype(&rsf_fg12_status) fg_status;
    decltype(&present_host) present_host;
    decltype(&rsf_d3d11_present_is_owner) present_is_owner;
    decltype(&rsf_d3d11_present_backend) present_backend;
    decltype(&rsf_d3d11_present_backend_choices) present_backend_choices;
    decltype(&rsf_d3d11_present_request) present_request;
    decltype(&rsf_d3d11_present_switch_result) present_switch_result;
    decltype(&rsf_d3d11_present_abort) present_abort;
    decltype(&pcl_message) pcl_message;
    decltype(&rsf_native_cpu_read) cpu_read;
    decltype(&latency_completed) latency_completed;
    decltype(&rsf_native_latency_end_session) latency_end_session;
    decltype(&rsf_input_dump_cancel) dump_cancel;
    decltype(&dlss_preset_valid) dlss_preset_valid;
    decltype(&fg_default_backend) fg_default_backend;
    decltype(&depth_domains_requested) depth_domains_requested;
};

constexpr Platform platform{
    gl_open, rsf_gl_graphics, rsf_gl_texture_create, rsf_gl_texture_get, rsf_gl_texture_destroy, rsf_gl_begin,
    rsf_gl_submit, rsf_gl_drain, rsf_gl_close, window_valid, window_create, window_destroy, window_child_of,
    window_resize, window_show, chain_create, chain_associate, chain_resize, chain_back_buffer, chain_present,
    adapter_identity, transition, copy_resource, rsf_sr12_create, rsf_sr12_plan, rsf_sr12_evaluate,
    rsf_sr12_set_sharpening, rsf_sr12_destroy, rsf_fg12_install, rsf_fg12_options, rsf_fg12_cpu, rsf_fg12_hudless,
    rsf_fg12_submitted, rsf_fg12_window, rsf_fg12_status, present_host, rsf_d3d11_present_is_owner,
    rsf_d3d11_present_backend, rsf_d3d11_present_backend_choices, rsf_d3d11_present_request,
    rsf_d3d11_present_switch_result, rsf_d3d11_present_abort,
    pcl_message, rsf_native_cpu_read, latency_completed, rsf_native_latency_end_session, rsf_input_dump_cancel,
    dlss_preset_valid, fg_default_backend, depth_domains_requested,
};
}

extern "C" rsf_backend_result rsf_core_gl_pipeline_open(const rsf_gl_pipeline_setup*, rsf_gl_pipeline**, const Platform*);

// Create on the parent window's owning thread. The window procedure and its event-thread routing
// are platform mechanisms, so these two entries stay native.
extern "C" void* rsf_gl_window_create(void* parent) { return window_create(parent); }
extern "C" int rsf_gl_window_destroy(void* window) { return window_destroy(window); }

extern "C" rsf_backend_result rsf_gl_pipeline_open(const rsf_gl_pipeline_setup* setup, rsf_gl_pipeline** out)
{
    return rsf_core_gl_pipeline_open(setup, out, &platform);
}
