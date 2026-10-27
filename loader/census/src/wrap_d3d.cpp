// SPDX-License-Identifier: GPL-3.0-only
// DXGI and D3D12: factory and device creation, command queues, swapchain creation and presentation.
//
// The factory, device, queue and swapchain methods are detoured at the address their vtable slot
// holds when the first object appears. Slot numbers come from the Windows SDK headers; the test in
// tests/vendor_census.cpp derives them again through a fake vtable and compares.
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <dxgi1_6.h>

#include "wrap_generic.h"
#include <rescaleframe/census_d3d12_observer.h>

namespace {
void (*device_observer)(void*){};
void (*before_submit_observer)(void*, uint32_t, void* const*){};
void (*submit_observer)(void*, uint32_t, void* const*){};
void (*present_observer)(void*){};
void (*swapchain_observer)(void*){};
void (*color_space_observer)(void*, uint32_t){};
}

extern "C" void rsf_census_observe_d3d12(void (*device)(void*),
    void (*before_submit)(void*, uint32_t, void* const*),
    void (*submit)(void*, uint32_t, void* const*), void (*present)(void*), void (*swapchain)(void*),
    void (*color_space)(void*, uint32_t))
{
    device_observer = device;
    before_submit_observer = before_submit;
    submit_observer = submit;
    present_observer = present;
    swapchain_observer = swapchain;
    color_space_observer = color_space;
}

namespace rsf_census {

namespace {

// Vtable slots (IUnknown 0..2, IDXGIObject 3..6).
constexpr size_t kFactoryCreateSwapChain = 10;
constexpr size_t kFactoryCreateSwapChainForHwnd = 15;
constexpr size_t kFactoryCreateSwapChainForCoreWindow = 16;
constexpr size_t kSwapChainPresent = 8;
constexpr size_t kSwapChainSetFullscreenState = 10;
constexpr size_t kSwapChainGetDesc = 12;
constexpr size_t kSwapChainResizeBuffers = 13;
constexpr size_t kSwapChainPresent1 = 22;
constexpr size_t kSwapChainGetFrameLatencyWaitableObject = 33;
constexpr size_t kSwapChainSetColorSpace1 = 38;
constexpr size_t kSwapChainResizeBuffers1 = 39;
constexpr size_t kDeviceCreateCommandQueue = 8;
constexpr size_t kQueueExecuteCommandLists = 10;
constexpr size_t kQueueGetDesc = 18;

std::atomic<int32_t> g_last_color_space{-1};

// Query `obj` for an interface and call `fn(iface)` with it, releasing afterwards.
template <class F>
void with_interface(void* obj, const GUID& iid, F&& fn) noexcept {
    using Qi = int32_t (*)(void*, const GUID&, void**);
    using Release = uint32_t (*)(void*);
    auto qi = reinterpret_cast<Qi>(vtable_slot(obj, 0));
    auto rel = reinterpret_cast<Release>(vtable_slot(obj, 2));
    if (!qi || !rel) return;
    void* iface = nullptr;
    bool ok = false;
    guard([&] { ok = qi(obj, iid, &iface) >= 0 && iface != nullptr; });
    if (!ok) return;
    fn(iface);
    guard([&] { rel(iface); });
}

void ensure_inline(Fn fn, void* obj, size_t slot, const char* why) noexcept {
    if (g_orig[idx(fn)].load(std::memory_order_acquire)) return;
    void* target = vtable_slot(obj, slot);
    if (target && bind_inline(fn, target, why)) hooks_apply();
}

void hook_factory(void* factory) noexcept {
    if (!factory) return;
    ensure_inline(Fn::DxgiFactoryCreateSwapChain, factory, kFactoryCreateSwapChain, "IDXGIFactory");
    with_interface(factory, __uuidof(IDXGIFactory2), [](void* f2) {
        ensure_inline(Fn::DxgiFactoryCreateSwapChainForHwnd, f2, kFactoryCreateSwapChainForHwnd, "IDXGIFactory2");
        ensure_inline(Fn::DxgiFactoryCreateSwapChainForCoreWindow, f2, kFactoryCreateSwapChainForCoreWindow,
                      "IDXGIFactory2");
    });
}

void hook_swapchain(void* sc) noexcept {
    if (swapchain_observer && sc) swapchain_observer(sc);
    if (!sc) return;
    ensure_inline(Fn::DxgiSwapChainPresent, sc, kSwapChainPresent, "IDXGISwapChain");
    ensure_inline(Fn::DxgiSwapChainSetFullscreenState, sc, kSwapChainSetFullscreenState, "IDXGISwapChain");
    ensure_inline(Fn::DxgiSwapChainResizeBuffers, sc, kSwapChainResizeBuffers, "IDXGISwapChain");
    with_interface(sc, __uuidof(IDXGISwapChain1), [](void* s) {
        ensure_inline(Fn::DxgiSwapChainPresent1, s, kSwapChainPresent1, "IDXGISwapChain1");
    });
    with_interface(sc, __uuidof(IDXGISwapChain2), [](void* s) {
        ensure_inline(Fn::DxgiSwapChainGetFrameLatencyWaitableObject, s, kSwapChainGetFrameLatencyWaitableObject,
                      "IDXGISwapChain2");
    });
    with_interface(sc, __uuidof(IDXGISwapChain3), [](void* s) {
        ensure_inline(Fn::DxgiSwapChainSetColorSpace1, s, kSwapChainSetColorSpace1, "IDXGISwapChain3");
        ensure_inline(Fn::DxgiSwapChainResizeBuffers1, s, kSwapChainResizeBuffers1, "IDXGISwapChain3");
    });
}

void put_guid_arg(Json& j, const char* key, const void* p) noexcept {
    uint8_t g[16];
    if (!plausible_ptr(p) || !safe_read(p, g, sizeof(g))) return;
    char s[48];
    uint32_t d1;
    uint16_t d2, d3;
    std::memcpy(&d1, g, 4);
    std::memcpy(&d2, g + 4, 2);
    std::memcpy(&d3, g + 6, 2);
    std::snprintf(s, sizeof(s), "%08X-%04X-%04X-%02X%02X-%02X%02X%02X%02X%02X%02X", d1, d2, d3, g[8], g[9], g[10],
                  g[11], g[12], g[13], g[14], g[15]);
    j.str(key, s);
}

// ---- Creation ---------------------------------------------------------------------------------
void dec_factory(Json& j, const Q* a) noexcept { put_guid_arg(j, "riid", reinterpret_cast<void*>(a[0])); }
void dec_factory2(Json& j, const Q* a) noexcept {
    j.hex("flags", static_cast<uint32_t>(a[0]));
    put_guid_arg(j, "riid", reinterpret_cast<void*>(a[1]));
}
void act_factory(const Q* a, Q ret) noexcept {
    if (static_cast<int32_t>(ret) >= 0 && plausible_ptr(reinterpret_cast<void*>(a[1]))) {
        void* f = nullptr;
        if (rd(reinterpret_cast<void*>(a[1]), f)) hook_factory(f);
    }
}
void act_factory2(const Q* a, Q ret) noexcept {
    if (static_cast<int32_t>(ret) >= 0 && plausible_ptr(reinterpret_cast<void*>(a[2]))) {
        void* f = nullptr;
        if (rd(reinterpret_cast<void*>(a[2]), f)) hook_factory(f);
    }
}
void post_factory(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "factory", a[1]); }
void post_factory2(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "factory", a[2]); }

// ---- D3D12 debug layer (RSF_CENSUS_FLAG_D3D12_DEBUG) ------------------------------------------
std::atomic<bool> g_debug_enabled{false};

// One record per message. The message ID stands in for the call site so each distinct message is
// recorded in full for its first occurrences instead of sharing one site's cap.
void CALLBACK debug_message(D3D12_MESSAGE_CATEGORY category, D3D12_MESSAGE_SEVERITY severity, D3D12_MESSAGE_ID id,
                           LPCSTR description, void* device) {
    if (severity > D3D12_MESSAGE_SEVERITY_WARNING) return;
    Call c(Fn::D3d12DebugMessage, reinterpret_cast<void*>(static_cast<uintptr_t>(0x10000u + id)));
    if (c.full()) {
        c.json().u64("id", static_cast<uint32_t>(id));
        c.json().hex("device", reinterpret_cast<uint64_t>(device));
        c.json().u64("category", static_cast<uint32_t>(category));
        static const char* const kSeverity[] = {"corruption", "error", "warning", "info", "message"};
        c.json().str("severity", static_cast<uint32_t>(severity) < 5 ? kSeverity[severity] : "unknown");
        char text[1024];
        if (description && safe_read_cstr(description, text, sizeof(text))) c.json().str("text", text);
    }
    c.finish(static_cast<uint32_t>(severity));
}

// Before the first device: the debug layer and DRED only apply to devices created afterwards.
void enable_debug_layer() noexcept {
    bool expected = false;
    if (!(option_flags() & (RSF_CENSUS_FLAG_D3D12_DEBUG | RSF_CENSUS_FLAG_D3D12_DEBUG_ONLY)) ||
        !g_debug_enabled.compare_exchange_strong(expected, true)) {
        return;
    }
    HMODULE d3d12 = GetModuleHandleW(L"d3d12.dll");
    using GetDebugInterface = HRESULT(WINAPI*)(REFIID, void**);
    auto get = d3d12 ? reinterpret_cast<GetDebugInterface>(
                           reinterpret_cast<void*>(GetProcAddress(d3d12, "D3D12GetDebugInterface")))
                     : nullptr;
    Call c(Fn::CensusEvent, reinterpret_cast<void*>(&enable_debug_layer));
    if (c.full()) c.json().str("event", "d3d12_debug_layer");
    HRESULT layer = E_NOINTERFACE, dred = E_NOINTERFACE;
    if (get) {
        ID3D12Debug* debug = nullptr;
        layer = get(IID_PPV_ARGS(&debug));
        if (SUCCEEDED(layer) && debug) {
            debug->EnableDebugLayer();
            debug->Release();
        }
        ID3D12DeviceRemovedExtendedDataSettings* settings = nullptr;
        dred = get(IID_PPV_ARGS(&settings));
        if (SUCCEEDED(dred) && settings) {
            settings->SetAutoBreadcrumbsEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            settings->SetPageFaultEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            settings->Release();
        }
    }
    if (c.full()) {
        c.json().hex("debug_layer_hr", static_cast<uint32_t>(layer));
        c.json().hex("dred_hr", static_cast<uint32_t>(dred));
    }
    c.finish(static_cast<uint32_t>(layer));
}

// The info queue is kept for the process lifetime: the callback must outlive every device message.
void attach_info_queue(void* device) noexcept {
    if (!g_debug_enabled.load(std::memory_order_acquire) || !device) return;
    ID3D12InfoQueue1* queue = nullptr;
    HRESULT hr = static_cast<IUnknown*>(device)->QueryInterface(IID_PPV_ARGS(&queue));
    DWORD cookie = 0;
    if (SUCCEEDED(hr) && queue) {
        queue->SetBreakOnSeverity(D3D12_MESSAGE_SEVERITY_CORRUPTION, FALSE);
        queue->SetBreakOnSeverity(D3D12_MESSAGE_SEVERITY_ERROR, FALSE);
        queue->SetBreakOnSeverity(D3D12_MESSAGE_SEVERITY_WARNING, FALSE);
        // A game's storage/retrieval filters can hide the first invalid command. Observation must
        // bypass those filters without changing the game's own filtering policy.
        hr = queue->RegisterMessageCallback(debug_message, D3D12_MESSAGE_CALLBACK_IGNORE_FILTERS, device, &cookie);
    }
    Call c(Fn::CensusEvent, reinterpret_cast<void*>(&attach_info_queue));
    if (c.full()) {
        c.json().str("event", "d3d12_info_queue");
        c.json().hex("device", reinterpret_cast<uint64_t>(device));
        c.json().hex("hr", static_cast<uint32_t>(hr));
        c.json().u64("cookie", cookie);
    }
    c.finish(static_cast<uint32_t>(hr));
}

void dec_create_device(Json& j, const Q* a) noexcept {
    j.hex("adapter", a[0]);
    j.hex("minimum_feature_level", static_cast<uint32_t>(a[1]));
    put_guid_arg(j, "riid", reinterpret_cast<void*>(a[2]));
    j.boolean("capability_test", a[3] == 0);
}
void post_create_device(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "device", a[3]); }
void act_create_device(const Q* a, Q ret) noexcept {
    if (static_cast<int32_t>(ret) < 0 || !plausible_ptr(reinterpret_cast<void*>(a[3]))) return;
    void* dev = nullptr;
    if (rd(reinterpret_cast<void*>(a[3]), dev)) {
        ensure_inline(Fn::D3d12DeviceCreateCommandQueue, dev, kDeviceCreateCommandQueue, "ID3D12Device");
        attach_info_queue(dev);
        if (device_observer) device_observer(dev);
    }
}

Q create_device(Q adapter, Q level, Q riid, Q out) noexcept {
    using Target = Q (*)(Q, Q, Q, Q);
    auto o = reinterpret_cast<Target>(orig<Fn::D3D12CreateDevice>());
    enable_debug_layer();
    Call c(Fn::D3D12CreateDevice, _ReturnAddress());
    const Q a[4] = {adapter, level, riid, out};
    if (c.full()) dec_create_device(c.json(), a);
    const Q r = o ? o(adapter, level, riid, out) : 0x80004005;
    if (c.full()) post_create_device(c.json(), a, r);
    act_create_device(a, r);
    c.finish(r);
    return r;
}

const char* queue_type_name(uint32_t t) noexcept {
    switch (t) {
    case 0: return "DIRECT";
    case 1: return "BUNDLE";
    case 2: return "COMPUTE";
    case 3: return "COPY";
    case 4: return "VIDEO_DECODE";
    case 5: return "VIDEO_PROCESS";
    case 6: return "VIDEO_ENCODE";
    default: return "other";
    }
}

void dec_create_queue(Json& j, const Q* a) noexcept {
    j.hex("device", a[0]);
    D3D12_COMMAND_QUEUE_DESC d;
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) && safe_read(reinterpret_cast<void*>(a[1]), &d, sizeof(d))) {
        j.u64("type", d.Type);
        j.str("type_name", queue_type_name(d.Type));
        j.i64("priority", d.Priority);
        j.hex("flags", d.Flags);
        j.u64("node_mask", d.NodeMask);
    }
    put_guid_arg(j, "riid", reinterpret_cast<void*>(a[2]));
}
void post_create_queue(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "queue", a[3]); }
void act_create_queue(const Q* a, Q ret) noexcept {
    if (static_cast<int32_t>(ret) < 0 || !plausible_ptr(reinterpret_cast<void*>(a[3]))) return;
    void* q = nullptr;
    D3D12_COMMAND_QUEUE_DESC d;
    if (!rd(reinterpret_cast<void*>(a[3]), q) || !q) return;
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) && safe_read(reinterpret_cast<void*>(a[1]), &d, sizeof(d))) {
        queue_describe(QueueKind::D3d12, q, d.Type, static_cast<uint32_t>(d.Priority), d.Flags);
    }
    ensure_inline(Fn::D3d12QueueExecuteCommandLists, q, kQueueExecuteCommandLists, "ID3D12CommandQueue");
}

void dec_execute(Json& j, const Q* a) noexcept {
    j.hex("queue", a[0]);
    const uint32_t n = static_cast<uint32_t>(a[1]);
    j.u64("num_lists", n);
    j.arr("lists");
    for (uint32_t i = 0; i < n && i < 8; ++i) {
        void* l;
        if (!plausible_ptr(reinterpret_cast<void*>(a[2])) ||
            !rd(reinterpret_cast<void**>(a[2]) + i, l)) {
            break;
        }
        j.hex(nullptr, reinterpret_cast<uint64_t>(l));
    }
    j.end_arr();
}
void begin_execute(const Q* a) noexcept {
    if (before_submit_observer)
        before_submit_observer(reinterpret_cast<void*>(a[0]), static_cast<uint32_t>(a[1]),
            reinterpret_cast<void* const*>(a[2]));
}
void act_execute(const Q* a, Q) noexcept {
    if (submit_observer) submit_observer(reinterpret_cast<void*>(a[0]), static_cast<uint32_t>(a[1]),
        reinterpret_cast<void* const*>(a[2]));
    const void* q = reinterpret_cast<void*>(a[0]);
    if (queue_submit(QueueKind::D3d12, q, static_cast<uint32_t>(a[1]))) {
        // A queue created before the census: read its description now.
        using GetDesc = D3D12_COMMAND_QUEUE_DESC* (*)(const void*, D3D12_COMMAND_QUEUE_DESC*);
        auto f = reinterpret_cast<GetDesc>(vtable_slot(q, kQueueGetDesc));
        D3D12_COMMAND_QUEUE_DESC d{};
        if (f && guard([&] { f(q, &d); })) queue_describe(QueueKind::D3d12, q, d.Type, static_cast<uint32_t>(d.Priority), d.Flags);
    }
}

// ---- Swapchain creation -----------------------------------------------------------------------
void put_desc1(Json& j, const char* key, const void* p) noexcept {
    DXGI_SWAP_CHAIN_DESC1 d;
    if (!plausible_ptr(p) || !safe_read(p, &d, sizeof(d))) return;
    j.obj(key);
    j.u64("width", d.Width);
    j.u64("height", d.Height);
    j.u64("format", d.Format);
    j.str("format_name", dxgi_format_name(d.Format));
    j.boolean("stereo", d.Stereo != 0);
    j.u64("samples", d.SampleDesc.Count);
    j.hex("buffer_usage", d.BufferUsage);
    j.u64("buffer_count", d.BufferCount);
    j.u64("scaling", d.Scaling);
    j.u64("swap_effect", d.SwapEffect);
    j.u64("alpha_mode", d.AlphaMode);
    j.hex("flags", d.Flags);
    j.end_obj();
}

void dec_create_swapchain(Json& j, const Q* a) noexcept {
    j.hex("factory", a[0]);
    j.hex("device_or_queue", a[1]);
    DXGI_SWAP_CHAIN_DESC d;
    if (plausible_ptr(reinterpret_cast<void*>(a[2])) && safe_read(reinterpret_cast<void*>(a[2]), &d, sizeof(d))) {
        j.obj("desc");
        j.u64("width", d.BufferDesc.Width);
        j.u64("height", d.BufferDesc.Height);
        j.u64("format", d.BufferDesc.Format);
        j.str("format_name", dxgi_format_name(d.BufferDesc.Format));
        j.u64("refresh_numerator", d.BufferDesc.RefreshRate.Numerator);
        j.u64("refresh_denominator", d.BufferDesc.RefreshRate.Denominator);
        j.u64("samples", d.SampleDesc.Count);
        j.hex("buffer_usage", d.BufferUsage);
        j.u64("buffer_count", d.BufferCount);
        j.hex("output_window", reinterpret_cast<uint64_t>(d.OutputWindow));
        j.boolean("windowed", d.Windowed != 0);
        j.u64("swap_effect", d.SwapEffect);
        j.hex("flags", d.Flags);
        j.end_obj();
    }
}
void post_create_swapchain(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "swapchain", a[3]); }
void act_create_swapchain(const Q* a, Q ret) noexcept {
    if (static_cast<int32_t>(ret) < 0 || !plausible_ptr(reinterpret_cast<void*>(a[3]))) return;
    void* sc = nullptr;
    if (rd(reinterpret_cast<void*>(a[3]), sc)) hook_swapchain(sc);
}

void dec_create_swapchain_hwnd(Json& j, const Q* a) noexcept {
    j.hex("factory", a[0]);
    j.hex("device_or_queue", a[1]);
    j.hex("hwnd", a[2]);
    put_desc1(j, "desc", reinterpret_cast<void*>(a[3]));
    DXGI_SWAP_CHAIN_FULLSCREEN_DESC fs;
    if (plausible_ptr(reinterpret_cast<void*>(a[4])) && safe_read(reinterpret_cast<void*>(a[4]), &fs, sizeof(fs))) {
        j.boolean("fullscreen_windowed", fs.Windowed != 0);
        j.u64("fullscreen_scaling", fs.Scaling);
    }
}
void post_create_swapchain_hwnd(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "swapchain", a[6]); }
void act_create_swapchain_hwnd(const Q* a, Q ret) noexcept {
    if (static_cast<int32_t>(ret) < 0 || !plausible_ptr(reinterpret_cast<void*>(a[6]))) return;
    void* sc = nullptr;
    if (rd(reinterpret_cast<void*>(a[6]), sc)) hook_swapchain(sc);
}
void dec_create_swapchain_cw(Json& j, const Q* a) noexcept {
    j.hex("factory", a[0]);
    j.hex("device_or_queue", a[1]);
    j.hex("core_window", a[2]);
    put_desc1(j, "desc", reinterpret_cast<void*>(a[3]));
}
void post_create_swapchain_cw(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "swapchain", a[5]); }
void act_create_swapchain_cw(const Q* a, Q ret) noexcept {
    if (static_cast<int32_t>(ret) < 0 || !plausible_ptr(reinterpret_cast<void*>(a[5]))) return;
    void* sc = nullptr;
    if (rd(reinterpret_cast<void*>(a[5]), sc)) hook_swapchain(sc);
}

// ---- Presentation -----------------------------------------------------------------------------
void put_present_flags(Json& j, uint32_t flags) noexcept {
    j.hex("present_flags", flags);
    j.arr("present_flag_names");
    if (flags & 0x1) j.str(nullptr, "TEST");
    if (flags & 0x2) j.str(nullptr, "DO_NOT_SEQUENCE");
    if (flags & 0x4) j.str(nullptr, "RESTART");
    if (flags & 0x8) j.str(nullptr, "DO_NOT_WAIT");
    if (flags & 0x10) j.str(nullptr, "STEREO_PREFER_RIGHT");
    if (flags & 0x20) j.str(nullptr, "STEREO_TEMPORARY_MONO");
    if (flags & 0x40) j.str(nullptr, "RESTRICT_TO_OUTPUT");
    if (flags & 0x100) j.str(nullptr, "USE_DURATION");
    if (flags & 0x200) j.str(nullptr, "ALLOW_TEARING");
    j.end_arr();
}

void put_swapchain_state(Json& j, const void* sc) noexcept {
    using GetDesc = int32_t (*)(const void*, DXGI_SWAP_CHAIN_DESC*);
    auto f = reinterpret_cast<GetDesc>(vtable_slot(sc, kSwapChainGetDesc));
    DXGI_SWAP_CHAIN_DESC d{};
    bool ok = false;
    if (f) guard([&] { ok = f(sc, &d) >= 0; });
    if (ok) {
        j.obj("swapchain_desc");
        j.u64("width", d.BufferDesc.Width);
        j.u64("height", d.BufferDesc.Height);
        j.u64("format", d.BufferDesc.Format);
        j.str("format_name", dxgi_format_name(d.BufferDesc.Format));
        j.u64("buffer_count", d.BufferCount);
        j.u64("swap_effect", d.SwapEffect);
        j.hex("flags", d.Flags);
        j.boolean("windowed", d.Windowed != 0);
        j.end_obj();
    }
    const int32_t cs = g_last_color_space.load(std::memory_order_relaxed);
    if (cs >= 0) j.u64("last_color_space_set", static_cast<uint32_t>(cs));
}

Q present(Q sc, Q sync, Q flags) noexcept {
    using Target = Q (*)(Q, Q, Q);
    auto o = reinterpret_cast<Target>(orig<Fn::DxgiSwapChainPresent>());
    PresentScope scope;
    if (scope.outer && !(flags & 0x1)) note_present_begin();
    if (scope.outer && !(flags & 0x1) && present_observer) present_observer(reinterpret_cast<void*>(sc));
    Call c(Fn::DxgiSwapChainPresent, _ReturnAddress());
    if (c.full()) {
        c.json().hex("swapchain", sc);
        c.json().u64("sync_interval", static_cast<uint32_t>(sync));
        put_present_flags(c.json(), static_cast<uint32_t>(flags));
        put_swapchain_state(c.json(), reinterpret_cast<void*>(sc));
    }
    const Q r = o ? o(sc, sync, flags) : 0;
    c.finish(r);
    return r;
}

Q present1(Q sc, Q sync, Q flags, Q params) noexcept {
    using Target = Q (*)(Q, Q, Q, Q);
    auto o = reinterpret_cast<Target>(orig<Fn::DxgiSwapChainPresent1>());
    PresentScope scope;
    if (scope.outer && !(flags & 0x1)) note_present_begin();
    Call c(Fn::DxgiSwapChainPresent1, _ReturnAddress());
    if (c.full()) {
        c.json().hex("swapchain", sc);
        c.json().u64("sync_interval", static_cast<uint32_t>(sync));
        put_present_flags(c.json(), static_cast<uint32_t>(flags));
        DXGI_PRESENT_PARAMETERS p;
        if (plausible_ptr(reinterpret_cast<void*>(params)) && safe_read(reinterpret_cast<void*>(params), &p, sizeof(p))) {
            c.json().u64("dirty_rects", p.DirtyRectsCount);
            c.json().boolean("scroll_rect", p.pScrollRect != nullptr);
        }
        put_swapchain_state(c.json(), reinterpret_cast<void*>(sc));
    }
    const Q r = o ? o(sc, sync, flags, params) : 0;
    c.finish(r);
    return r;
}

void dec_resize(Json& j, const Q* a) noexcept {
    j.hex("swapchain", a[0]);
    j.u64("buffer_count", static_cast<uint32_t>(a[1]));
    j.u64("width", static_cast<uint32_t>(a[2]));
    j.u64("height", static_cast<uint32_t>(a[3]));
    j.u64("format", static_cast<uint32_t>(a[4]));
    j.str("format_name", dxgi_format_name(static_cast<uint32_t>(a[4])));
    j.hex("flags", static_cast<uint32_t>(a[5]));
}
void dec_color_space(Json& j, const Q* a) noexcept {
    j.hex("swapchain", a[0]);
    j.u64("color_space", static_cast<uint32_t>(a[1]));
}
void act_color_space(const Q* a, Q ret) noexcept {
    if (static_cast<int32_t>(ret) >= 0) g_last_color_space.store(static_cast<int32_t>(a[1]));
    if (static_cast<int32_t>(ret) >= 0 && color_space_observer)
        color_space_observer(reinterpret_cast<void*>(a[0]), static_cast<uint32_t>(a[1]));
}
void dec_fullscreen(Json& j, const Q* a) noexcept {
    j.hex("swapchain", a[0]);
    j.boolean("fullscreen", static_cast<uint32_t>(a[1]) != 0);
    j.hex("target_output", a[2]);
}
void dec_waitable(Json& j, const Q* a) noexcept { j.hex("swapchain", a[0]); }

} // namespace

void register_wrappers_d3d(void** t) noexcept {
    RSF_REG(t, CreateDXGIFactory, dec_factory, post_factory, act_factory);
    RSF_REG(t, CreateDXGIFactory1, dec_factory, post_factory, act_factory);
    RSF_REG(t, CreateDXGIFactory2, dec_factory2, post_factory2, act_factory2);
    t[idx(Fn::D3D12CreateDevice)] = reinterpret_cast<void*>(&create_device);
    RSF_REG(t, DxgiFactoryCreateSwapChain, dec_create_swapchain, post_create_swapchain, act_create_swapchain);
    RSF_REG(t, DxgiFactoryCreateSwapChainForHwnd, dec_create_swapchain_hwnd, post_create_swapchain_hwnd,
            act_create_swapchain_hwnd);
    RSF_REG(t, DxgiFactoryCreateSwapChainForCoreWindow, dec_create_swapchain_cw, post_create_swapchain_cw,
            act_create_swapchain_cw);
    t[idx(Fn::DxgiSwapChainPresent)] = reinterpret_cast<void*>(&present);
    t[idx(Fn::DxgiSwapChainPresent1)] = reinterpret_cast<void*>(&present1);
    RSF_REG(t, DxgiSwapChainResizeBuffers, dec_resize);
    RSF_REG(t, DxgiSwapChainResizeBuffers1, dec_resize);
    RSF_REG(t, DxgiSwapChainSetColorSpace1, dec_color_space, nullptr, act_color_space);
    RSF_REG(t, DxgiSwapChainSetFullscreenState, dec_fullscreen);
    RSF_REG(t, DxgiSwapChainGetFrameLatencyWaitableObject, dec_waitable);
    RSF_REG(t, D3d12DeviceCreateCommandQueue, dec_create_queue, post_create_queue, act_create_queue);
    RSF_REG(t, D3d12QueueExecuteCommandLists, dec_execute, nullptr, act_execute, begin_execute);
}

} // namespace rsf_census
