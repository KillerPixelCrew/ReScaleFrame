// SPDX-License-Identifier: MIT
// IDXGISwapChain4 facade and native leaves of the shared presentation bridge. The Rust owner in
// runtime/orchestrator/src/d3d11_present.rs decides admission, publication, provider switching,
// frame and slot sequencing, Present, fallback, resize, retirement, status and latency routing.
// Each leaf here is one DXGI, D3D12, D3D11, Win32, MinHook or SDK step on this object.
#include <rescaleframe/d3d11_present_bridge.h>
#include <rescaleframe/shared_surface.h>
#include "../../backends/common/d3d12_helpers.h"
#include "d3d11_present_platform.h"
#include <d3d11_4.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <atomic>
#include <cstddef>
#include <cstdio>
#include <memory>
#include <new>

static_assert(sizeof(rsf_streamline_graphics) == 48);
static_assert(sizeof(rsf_d3d11p_request) == 48 && offsetof(rsf_d3d11p_request, refresh_denominator) == 44);
static_assert(sizeof(rsf_d3d11p_facts) == 112 && offsetof(rsf_d3d11p_facts, graphics) == 40 &&
    offsetof(rsf_d3d11p_facts, direct_queue) == 88 && offsetof(rsf_d3d11p_facts, device11) == 96 &&
    offsetof(rsf_d3d11p_facts, buffer_usage) == 104);
static_assert(sizeof(rsf_d3d11p_plain_plan) == 24 && offsetof(rsf_d3d11p_plain_plan, swap_effect) == 8);
static_assert(sizeof(rsf_d3d11p_adapter) == 16);
static_assert(sizeof(rsf_d3d11p_platform) == 72 * sizeof(void*));
static_assert(offsetof(rsf_d3d11p_platform, bridge_new) == 15 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, init_source) == 19 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, create_interop_queue) == 29 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, close_event) == 33 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, shared_surface_platform) == 36 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, create_plain) == 38 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, join) == 44 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, acquire_backbuffer) == 47 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, query_buffer_interface) == 56 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, signal_wait) == 59 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, set_description) == 62 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, create_retirement_fence) == 63 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, release_direct_graphics) == 67 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, release_host) == 68 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, verify_outer) == 71 * sizeof(void*));
static_assert(sizeof(rsf_d3d11_present_status) == 280 && sizeof(rsf_observer_present_event) == 32);
static_assert(DXGI_FORMAT_R8G8B8A8_UNORM == 28 && DXGI_FORMAT_B8G8R8A8_UNORM == 87 &&
    DXGI_FORMAT_R10G10B10A2_UNORM == 24 && DXGI_FORMAT_R16G16B16A16_FLOAT == 10);
static_assert(DXGI_PRESENT_TEST == 1 && DXGI_PRESENT_ALLOW_TEARING == 0x200 &&
    DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT == 64 && DXGI_USAGE_UNORDERED_ACCESS == 0x400);
static_assert(DXGI_SWAP_EFFECT_FLIP_DISCARD == 4 && DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING == 2048 &&
    DXGI_USAGE_RENDER_TARGET_OUTPUT == 0x20);
static_assert(RSF_FG_BACKEND_DLSS == 1 && RSF_FG_BACKEND_FSR3 == 3 && RSF_FG_BACKEND_FSR4 == 4 &&
    RSF_FG_BACKEND_XESS == 5 && RSF_FG_BACKEND_AUTO == 7 && RSF_PACING_ANTILAG2 == 3 &&
    RSF_LATENCY_INPUT_SAMPLE == 6 && RSF_UI_MODE_NONE == 1);

namespace {
using Microsoft::WRL::ComPtr;
using Create = HRESULT(STDMETHODCALLTYPE*)(IDXGIFactory*, IUnknown*, DXGI_SWAP_CHAIN_DESC*, IDXGISwapChain**);
using CreateHwnd = HRESULT(STDMETHODCALLTYPE*)(IDXGIFactory2*, IUnknown*, HWND,
    const DXGI_SWAP_CHAIN_DESC1*, const DXGI_SWAP_CHAIN_FULLSCREEN_DESC*, IDXGIOutput*, IDXGISwapChain1**);
Create original_create = nullptr;
CreateHwnd original_hwnd = nullptr;

const rsf_d3d11_present_setup& settings() { return *rsf_core_d3d11p_settings(); }
void log(const char* message) { if (settings().log) settings().log(settings().user, message); }
bool switching() { return settings().runtime_switching != 0; }
bool same_identity(IUnknown* left, IUnknown* right) {
    if (!left || !right) return false;
    ComPtr<IUnknown> a,b;
    return SUCCEEDED(left->QueryInterface(IID_PPV_ARGS(&a))) &&
        SUCCEEDED(right->QueryInterface(IID_PPV_ARGS(&b))) && a.Get() == b.Get();
}

class PhysicalLease {
    const rsf_d3d11p_physical_lease* lease = nullptr;
    void* physical = nullptr;
    HRESULT result;
public:
    explicit PhysicalLease(rsf_d3d11p_chain* chain, bool initialize = false)
        : result(rsf_core_d3d11p_acquire_physical(chain, initialize ? 1u : 0u, &lease, &physical)) {}
    ~PhysicalLease() { rsf_core_d3d11p_release_physical(lease); }
    PhysicalLease(const PhysicalLease&) = delete;
    PhysicalLease& operator=(const PhysicalLease&) = delete;
    IDXGISwapChain4* get() const { return static_cast<IDXGISwapChain4*>(physical); }
    HRESULT status() const { return result; }
};

struct Bridge final : public IDXGISwapChain4 {
    std::atomic<ULONG> references{1};
    rsf_d3d11p_chain* chain;
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext4> context;
    ComPtr<IDXGIAdapter> adapter;
    ComPtr<IDXGIFactory> parent;
    bool tearing_supported = false;
    rsf_streamline_host* host = nullptr;
    rsf_streamline_graphics graphics{};
    ComPtr<ID3D12Device> direct_device;
    ComPtr<ID3D12CommandQueue> direct_queue;
    bool native12 = false;
    rsf_shared_fence* shared_fence = nullptr;
    // The three slot allocator/list pairs are leased by the Rust owner, which releases them
    // where these members were destroyed.
    DXGI_SWAP_CHAIN_DESC description{};

    explicit Bridge(rsf_d3d11p_chain* owner) : chain(owner) {}
    ~Bridge() { rsf_core_d3d11p_chain_destroy(chain); }

    template<class Call> HRESULT query_physical(Call call) {
        const PhysicalLease lease(chain, true);
        return lease.get() ? call(lease.get()) : lease.status();
    }
    // Streamline's proxy queue takes the proxy list; any other provider queue takes the native
    // list behind it. Null when the host cannot unwrap the list.
    void* provider_list(void* queue, ID3D12GraphicsCommandList* list) {
        return host && queue != graphics.queue ? rsf_streamline_host_native(host, list) : list;
    }
    // Streamline needs the engine identity; without one there is no host and no DLSS-G.
    rsf_streamline_host_setup host_setup(void* dxgi_adapter) const {
        const auto& s = settings();
        return {sizeof(rsf_streamline_host_setup), RSF_STREAMLINE_HOST_ABI_VERSION, s.streamline_directory_utf8, dxgi_adapter,
            s.engine_type, s.engine_version_utf8, s.project_id_utf8, 1, s.development_runtime,
            s.log, s.user, RSF_SL_PROFILE_DLSS_FG, 1};
    }

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void** out) override {
        if (!out) return E_POINTER;
        *out = nullptr;
        if (iid == __uuidof(IUnknown) || iid == __uuidof(IDXGIObject) || iid == __uuidof(IDXGIDeviceSubObject) ||
            iid == __uuidof(IDXGISwapChain) || iid == __uuidof(IDXGISwapChain1) || iid == __uuidof(IDXGISwapChain2) ||
            iid == __uuidof(IDXGISwapChain3) || iid == __uuidof(IDXGISwapChain4)) {
            *out = static_cast<IDXGISwapChain4*>(this); AddRef(); return S_OK;
        }
        return E_NOINTERFACE;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++references; }
    ULONG STDMETHODCALLTYPE Release() override {
        const volatile ULONG result = release_owned(); return result;
    }
    __declspec(noinline) ULONG release_owned() {
        const auto value = --references; if (!value) delete this; return value;
    }
    HRESULT STDMETHODCALLTYPE SetPrivateData(REFGUID key, UINT bytes, const void* data) override { return query_physical([&](IDXGISwapChain4* c) { return c->SetPrivateData(key, bytes, data); }); }
    HRESULT STDMETHODCALLTYPE SetPrivateDataInterface(REFGUID key, const IUnknown* data) override { return query_physical([&](IDXGISwapChain4* c) { return c->SetPrivateDataInterface(key, data); }); }
    HRESULT STDMETHODCALLTYPE GetPrivateData(REFGUID key, UINT* bytes, void* data) override { return query_physical([&](IDXGISwapChain4* c) { return c->GetPrivateData(key, bytes, data); }); }
    HRESULT STDMETHODCALLTYPE GetParent(REFIID iid, void** out) override { return parent->QueryInterface(iid, out); }
    HRESULT STDMETHODCALLTYPE GetDevice(REFIID iid, void** out) override { return native12 ? direct_device->QueryInterface(iid, out) : device->QueryInterface(iid, out); }
    HRESULT STDMETHODCALLTYPE Present(UINT sync, UINT flags) override {
        // Keep a real call/return boundary: external hook installers may follow
        // an entry tail jump and otherwise detour the private continuation too.
        const volatile HRESULT result = present_owned(sync, flags);
        return result;
    }
    // A downstream wrapper continuation must not reenter the detoured virtual entry.
    __declspec(noinline) HRESULT present_owned(UINT sync, UINT flags) { return rsf_core_d3d11p_present(chain, sync, flags); }
    HRESULT STDMETHODCALLTYPE GetBuffer(UINT index, REFIID iid, void** out) override {
        return rsf_core_d3d11p_get_buffer(chain, index, &iid, out);
    }
    HRESULT STDMETHODCALLTYPE SetFullscreenState(BOOL enabled, IDXGIOutput* target) override {
        const volatile HRESULT result = set_fullscreen_owned(enabled,target); return result;
    }
    __declspec(noinline) HRESULT set_fullscreen_owned(BOOL enabled, IDXGIOutput* target) {
        return rsf_core_d3d11p_set_fullscreen(chain, enabled, target);
    }
    HRESULT STDMETHODCALLTYPE GetFullscreenState(BOOL* enabled, IDXGIOutput** target) override {
        if (rsf_core_d3d11p_physical_pending(chain)) {
            if (!enabled && !target) return E_INVALIDARG;
            if (enabled) *enabled = !description.Windowed;
            if (target) *target = nullptr;
            return S_OK;
        }
        return query_physical([&](IDXGISwapChain4* c) { return c->GetFullscreenState(enabled, target); });
    }
    HRESULT STDMETHODCALLTYPE GetDesc(DXGI_SWAP_CHAIN_DESC* out) override { if (!out) return E_POINTER; if (native12 && !switching()) return query_physical([&](IDXGISwapChain4* c) { return c->GetDesc(out); }); *out = description; return S_OK; }
    HRESULT STDMETHODCALLTYPE ResizeBuffers(UINT count, UINT width, UINT height, DXGI_FORMAT format, UINT flags) override {
        const volatile HRESULT result = resize_buffers_owned(count,width,height,format,flags); return result;
    }
    __declspec(noinline) HRESULT resize_buffers_owned(UINT count, UINT width, UINT height, DXGI_FORMAT format, UINT flags) {
        return rsf_core_d3d11p_resize(chain, count, width, height, uint32_t(format), flags);
    }
    HRESULT STDMETHODCALLTYPE ResizeTarget(const DXGI_MODE_DESC* desc) override {
        // The physical chain is created lazily; like every other window call, ResizeTarget creates it first instead of
        // failing with DXGI_ERROR_WAS_STILL_DRAWING when an engine resizes before its first Present.
        return query_physical([&](IDXGISwapChain4* c) { return c->ResizeTarget(desc); });
    }
    HRESULT STDMETHODCALLTYPE GetContainingOutput(IDXGIOutput** out) override { return query_physical([&](IDXGISwapChain4* c) { return c->GetContainingOutput(out); }); }
    HRESULT STDMETHODCALLTYPE GetFrameStatistics(DXGI_FRAME_STATISTICS* out) override {
        const auto result = query_physical([&](IDXGISwapChain4* c) { return c->GetFrameStatistics(out); });
        if (SUCCEEDED(result) && out && native12 && switching()) out->PresentCount = rsf_core_d3d11p_application_presents(chain);
        return result;
    }
    HRESULT STDMETHODCALLTYPE GetLastPresentCount(UINT* out) override {
        if ((native12 && switching()) || rsf_core_d3d11p_physical_pending(chain)) {
            if (!out) return E_POINTER; *out = rsf_core_d3d11p_application_presents(chain); return S_OK;
        }
        return query_physical([&](IDXGISwapChain4* c) { return c->GetLastPresentCount(out); });
    }
    HRESULT STDMETHODCALLTYPE GetDesc1(DXGI_SWAP_CHAIN_DESC1* out) override {
        if (!out) return E_POINTER;
        if (!native12 || switching()) {
            *out = {description.BufferDesc.Width,description.BufferDesc.Height,description.BufferDesc.Format,FALSE,
                description.SampleDesc,description.BufferUsage,description.BufferCount,DXGI_SCALING_STRETCH,
                description.SwapEffect,DXGI_ALPHA_MODE_UNSPECIFIED,description.Flags};
            return S_OK;
        }
        return query_physical([&](IDXGISwapChain4* c) { return c->GetDesc1(out); });
    }
    HRESULT STDMETHODCALLTYPE GetFullscreenDesc(DXGI_SWAP_CHAIN_FULLSCREEN_DESC* out) override {
        if (!out) return E_POINTER;
        if (!native12 || switching()) {
            *out = {description.BufferDesc.RefreshRate,description.BufferDesc.ScanlineOrdering,
                description.BufferDesc.Scaling,description.Windowed};
            return S_OK;
        }
        return query_physical([&](IDXGISwapChain4* c) { return c->GetFullscreenDesc(out); });
    }
    HRESULT STDMETHODCALLTYPE GetHwnd(HWND* out) override { if (!out) return E_POINTER; *out = description.OutputWindow; return S_OK; }
    HRESULT STDMETHODCALLTYPE GetCoreWindow(REFIID, void** out) override { if (out) *out = nullptr; return E_NOINTERFACE; }
    HRESULT STDMETHODCALLTYPE Present1(UINT sync, UINT flags, const DXGI_PRESENT_PARAMETERS* params) override {
        const volatile HRESULT result = present1_owned(sync,flags,params); return result;
    }
    __declspec(noinline) HRESULT present1_owned(UINT sync, UINT flags, const DXGI_PRESENT_PARAMETERS* params) {
        if (params && (params->DirtyRectsCount || params->pScrollRect)) return DXGI_ERROR_INVALID_CALL;
        return present_owned(sync, flags);
    }
    BOOL STDMETHODCALLTYPE IsTemporaryMonoSupported() override {
        const PhysicalLease lease(chain);
        return lease.get() ? lease.get()->IsTemporaryMonoSupported() : FALSE;
    }
    HRESULT STDMETHODCALLTYPE GetRestrictToOutput(IDXGIOutput** out) override { return query_physical([&](IDXGISwapChain4* c) { return c->GetRestrictToOutput(out); }); }
    HRESULT STDMETHODCALLTYPE SetBackgroundColor(const DXGI_RGBA* color) override { return query_physical([&](IDXGISwapChain4* c) { return c->SetBackgroundColor(color); }); }
    HRESULT STDMETHODCALLTYPE GetBackgroundColor(DXGI_RGBA* color) override { return query_physical([&](IDXGISwapChain4* c) { return c->GetBackgroundColor(color); }); }
    HRESULT STDMETHODCALLTYPE SetRotation(DXGI_MODE_ROTATION value) override { return query_physical([&](IDXGISwapChain4* c) { return c->SetRotation(value); }); }
    HRESULT STDMETHODCALLTYPE GetRotation(DXGI_MODE_ROTATION* value) override { return query_physical([&](IDXGISwapChain4* c) { return c->GetRotation(value); }); }
    HRESULT STDMETHODCALLTYPE SetSourceSize(UINT width, UINT height) override { return query_physical([&](IDXGISwapChain4* c) { return c->SetSourceSize(width, height); }); }
    HRESULT STDMETHODCALLTYPE GetSourceSize(UINT* width, UINT* height) override { return query_physical([&](IDXGISwapChain4* c) { return c->GetSourceSize(width, height); }); }
    HRESULT STDMETHODCALLTYPE SetMaximumFrameLatency(UINT value) override {
        if (native12 && switching()) return rsf_core_d3d11p_set_max_latency(chain, value);
        return native12 ? query_physical([&](IDXGISwapChain4* c) { return c->SetMaximumFrameLatency(value); }) : DXGI_ERROR_INVALID_CALL;
    }
    HRESULT STDMETHODCALLTYPE GetMaximumFrameLatency(UINT* out) override {
        if (!out) return E_POINTER;
        if (native12 && switching()) return rsf_core_d3d11p_max_latency(chain, out);
        if (native12) return query_physical([&](IDXGISwapChain4* c) { return c->GetMaximumFrameLatency(out); });
        *out = 1; return S_OK;
    }
    HANDLE STDMETHODCALLTYPE GetFrameLatencyWaitableObject() override {
        if (native12 && switching()) {
            // The application owns this duplicate. Provider replacement must never close
            // the wait object retained by the engine for the facade's entire lifetime.
            return rsf_core_d3d11p_duplicate_application_ready(chain);
        }
        if (!native12) return nullptr;
        const PhysicalLease lease(chain);
        return lease.get() ? lease.get()->GetFrameLatencyWaitableObject() : nullptr;
    }
    HRESULT STDMETHODCALLTYPE SetMatrixTransform(const DXGI_MATRIX_3X2_F* value) override { return query_physical([&](IDXGISwapChain4* c) { return c->SetMatrixTransform(value); }); }
    HRESULT STDMETHODCALLTYPE GetMatrixTransform(DXGI_MATRIX_3X2_F* value) override { return query_physical([&](IDXGISwapChain4* c) { return c->GetMatrixTransform(value); }); }
    UINT STDMETHODCALLTYPE GetCurrentBackBufferIndex() override {
        // DX11 retains a view of logical buffer zero across Present. Our shared textures
        // cannot perform DXGI's automatic backing-resource rotation, so keep that source.
        if (!native12) return 0;
        if (switching()) return rsf_core_d3d11p_render_index(chain);
        const PhysicalLease lease(chain);
        return lease.get() ? lease.get()->GetCurrentBackBufferIndex() : 0;
    }
    HRESULT STDMETHODCALLTYPE CheckColorSpaceSupport(DXGI_COLOR_SPACE_TYPE value, UINT* out) override { return query_physical([&](IDXGISwapChain4* c) { return c->CheckColorSpaceSupport(value, out); }); }
    HRESULT STDMETHODCALLTYPE SetColorSpace1(DXGI_COLOR_SPACE_TYPE value) override {
        const volatile HRESULT result = set_color_space_owned(value); return result;
    }
    __declspec(noinline) HRESULT set_color_space_owned(DXGI_COLOR_SPACE_TYPE value) {
        return query_physical([&](IDXGISwapChain4* c) { return c->SetColorSpace1(value); });
    }
    HRESULT STDMETHODCALLTYPE ResizeBuffers1(UINT count, UINT width, UINT height, DXGI_FORMAT format, UINT flags, const UINT* masks, IUnknown* const* queues) override {
        const volatile HRESULT result = resize_buffers1_owned(count,width,height,format,flags,masks,queues); return result;
    }
    __declspec(noinline) HRESULT resize_buffers1_owned(UINT count, UINT width, UINT height, DXGI_FORMAT format, UINT flags, const UINT* masks, IUnknown* const* queues) {
        if (masks || queues) return DXGI_ERROR_INVALID_CALL;
        return resize_buffers_owned(count,width,height,format,flags);
    }
    HRESULT STDMETHODCALLTYPE SetHDRMetaData(DXGI_HDR_METADATA_TYPE type, UINT bytes, void* data) override { return query_physical([&](IDXGISwapChain4* c) { return c->SetHDRMetaData(type, bytes, data); }); }
};

Bridge* as_bridge(void* value) { return static_cast<Bridge*>(static_cast<IDXGISwapChain4*>(value)); }
void* as_object(Bridge* bridge) { return static_cast<IDXGISwapChain4*>(bridge); }

// Leaves. Each one is a native step on one facade; none calls back into the Rust owner.
namespace leaf {
const rsf_generation_provider* provider_table(uint32_t family) {
    switch (family) {
    case RSF_D3D11P_FAMILY_DLSS: return rsf_generation_dlss();
    case RSF_D3D11P_FAMILY_FIDELITYFX: return rsf_generation_fsr();
    case RSF_D3D11P_FAMILY_XESS: return rsf_generation_xess();
    default: return nullptr;
    }
}
void observer_present(rsf_observer_present_event* event) { (void)rsf_observer_notify_application_present(event); }
uint32_t window_valid(void* window) { return IsWindow(static_cast<HWND>(window)) ? 1u : 0u; }
uint32_t client_size(void* window, uint32_t* width, uint32_t* height) {
    RECT rect{}; if (!GetClientRect(static_cast<HWND>(window), &rect)) return 0;
    *width = UINT(rect.right); *height = UINT(rect.bottom); return 1;
}
void* canonical_identity(void* receiver) {
    ComPtr<IUnknown> identity;
    return SUCCEEDED(static_cast<IUnknown*>(receiver)->QueryInterface(IID_PPV_ARGS(&identity)))
        ? identity.Get() : nullptr;
}
void add_ref(void* object) { static_cast<IUnknown*>(object)->AddRef(); }
void release(void* object) { static_cast<IUnknown*>(object)->Release(); }
rsf_streamline_host* sl(void* host) { return static_cast<rsf_streamline_host*>(host); }
int32_t host_sleep(void* host, uint64_t id) { return rsf_streamline_host_sleep(sl(host), id); }
int32_t host_marker(void* host, uint64_t id, uint32_t marker, uint32_t controller) { return rsf_streamline_host_marker(sl(host), id, marker, controller); }
int32_t host_acquire(void* host, uint64_t id) { return rsf_streamline_host_acquire(sl(host), id); }
int32_t host_input(void* host, uint64_t id, uint32_t kinds, uint32_t message) { return rsf_streamline_host_input(sl(host), id, kinds, message); }
int32_t host_abort(void* host, uint64_t id) { return rsf_streamline_host_abort(sl(host), id); }
int32_t host_reflex_off(void* host) { return rsf_streamline_host_reflex(sl(host), RSF_REFLEX_OFF, 0); }
int32_t host_generation_load(void* host, uint32_t enabled) { return rsf_streamline_host_generation_load(sl(host), enabled); }
int32_t host_present_common(void* host, void* chain, uint32_t sync, uint32_t flags, uint32_t begin) {
    return rsf_streamline_host_present_common(sl(host), chain, sync, flags, begin);
}

void* bridge_new(rsf_d3d11p_chain* chain) {
    auto* bridge = new(std::nothrow) Bridge(chain);
    return bridge ? as_object(bridge) : nullptr;
}
void bridge_release_owned(void* b) { as_bridge(b)->release_owned(); }
rsf_d3d11p_chain* bridge_chain(void* b) { return as_bridge(b)->chain; }
void facts(void* b, rsf_d3d11p_facts* out) {
    const auto& self = *as_bridge(b); const auto& d = self.description;
    *out = {d.OutputWindow, d.BufferDesc.Width, d.BufferDesc.Height, uint32_t(d.BufferDesc.Format), d.BufferCount,
        d.Flags, d.Windowed ? 1u : 0u, self.native12 ? 1u : 0u, self.tearing_supported ? 1u : 0u, self.graphics,
        self.direct_queue.Get(), self.device.Get(), d.BufferUsage};
}

uint32_t init_source(void* b, void* factory, void* source, const void* desc) {
    auto& self = *as_bridge(b);
    self.description = *static_cast<const DXGI_SWAP_CHAIN_DESC*>(desc);
    self.parent = static_cast<IDXGIFactory*>(factory);
    self.graphics.struct_size = sizeof(self.graphics);
    self.native12 = SUCCEEDED(static_cast<IUnknown*>(source)->QueryInterface(IID_PPV_ARGS(&self.direct_queue)));
    return self.native12 ? 1u : 0u;
}
int32_t owning_adapter(void* b, void* source, rsf_d3d11p_adapter* out) {
    auto& self = *as_bridge(b);
    ComPtr<IDXGIFactory4> owning_factory;
    ComPtr<IDXGIAdapter1> owning_adapter;
    if (self.native12) {
        if (FAILED(self.direct_queue->GetDevice(IID_PPV_ARGS(&self.direct_device))) ||
            FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&owning_factory))) ||
            FAILED(owning_factory->EnumAdapterByLuid(self.direct_device->GetAdapterLuid(), IID_PPV_ARGS(&owning_adapter)))) return E_FAIL;
    } else {
        ComPtr<IDXGIDevice> dxgi; ComPtr<IDXGIAdapter> adapter;
        if (FAILED(static_cast<IUnknown*>(source)->QueryInterface(IID_PPV_ARGS(&dxgi))) || FAILED(dxgi->GetAdapter(&adapter)) ||
            FAILED(adapter.As(&owning_adapter))) return E_FAIL;
    }
    DXGI_ADAPTER_DESC1 info{};
    if (FAILED(owning_adapter->GetDesc1(&info))) return E_FAIL;
    *out = {info.VendorId, info.DeviceId, rsf_fg_default_backend(info.VendorId, (info.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0), 0};
    return S_OK;
}
int32_t native_device(void* b) {
    auto& self = *as_bridge(b);
    return FAILED(self.direct_queue->GetDevice(IID_PPV_ARGS(&self.direct_device))) ? E_NOINTERFACE : S_OK;
}
uint32_t adopt_host(void* b, uint32_t identity, int32_t* result) {
    auto& self = *as_bridge(b);
    ComPtr<IDXGIFactory4> native_factory; ComPtr<IDXGIAdapter1> native_adapter;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&native_factory))) ||
        FAILED(native_factory->EnumAdapterByLuid(self.direct_device->GetAdapterLuid(), IID_PPV_ARGS(&native_adapter)))) return 0;
    const auto setup = self.host_setup(native_adapter.Get());
    *result = identity ? rsf_streamline_host_adopt(&setup, self.direct_device.Get(), self.direct_queue.Get(), &self.host) :
        RSF_BACKEND_ERROR_NOT_SUPPORTED;
    return 1;
}
int32_t d3d11_device(void* b, void* source) {
    auto& self = *as_bridge(b);
    if (FAILED(static_cast<IUnknown*>(source)->QueryInterface(IID_PPV_ARGS(&self.device)))) return E_NOINTERFACE;
    ComPtr<ID3D11DeviceContext> immediate; self.device->GetImmediateContext(&immediate);
    ComPtr<IDXGIDevice> dxgi;
    if (FAILED(immediate.As(&self.context)) || FAILED(self.device.As(&dxgi)) || FAILED(dxgi->GetAdapter(&self.adapter))) return E_NOINTERFACE;
    return S_OK;
}
int32_t create_host(void* b) {
    auto& self = *as_bridge(b);
    const auto setup = self.host_setup(self.adapter.Get());
    return rsf_streamline_host_create(&setup, &self.host);
}
void* host(void* b) { return as_bridge(b)->host; }
// The adapter is the last initialization input; like the former local, it is not kept.
int32_t resolve_graphics(void* b) {
    auto& self = *as_bridge(b);
    const auto adapter = std::move(self.adapter);
    if (self.host) return rsf_streamline_host_graphics(self.host, &self.graphics) != RSF_BACKEND_OK ? E_FAIL : S_OK;
    if (!self.native12 && FAILED(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0,
            IID_PPV_ARGS(&self.direct_device)))) return DXGI_ERROR_UNSUPPORTED;
    if (!self.direct_queue) {
        D3D12_COMMAND_QUEUE_DESC queue{}; queue.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
        if (FAILED(self.direct_device->CreateCommandQueue(&queue, IID_PPV_ARGS(&self.direct_queue)))) return E_FAIL;
    }
    self.graphics.device = self.graphics.native_device = self.direct_device.Get();
    self.graphics.queue = self.graphics.native_queue = self.direct_queue.Get();
    return S_OK;
}
uint32_t tearing_supported(void* b) {
    auto& self = *as_bridge(b);
    ComPtr<IDXGIFactory5> capability_factory;
    BOOL allow_tearing = FALSE;
    self.tearing_supported = SUCCEEDED(self.parent->QueryInterface(IID_PPV_ARGS(&capability_factory))) &&
        SUCCEEDED(capability_factory->CheckFeatureSupport(DXGI_FEATURE_PRESENT_ALLOW_TEARING,
            &allow_tearing, sizeof(allow_tearing))) && allow_tearing;
    return self.tearing_supported ? 1u : 0u;
}
uint32_t fsr4_supported(void* b, const char* directory) {
    return rsf_generation_fsr4_supported(as_bridge(b)->graphics.native_device, directory);
}
int32_t create_interop_queue(void* device, void** out) {
    D3D12_COMMAND_QUEUE_DESC interop_desc{}; interop_desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    ID3D12CommandQueue* queue = nullptr;
    const auto result = static_cast<ID3D12Device*>(device)->CreateCommandQueue(&interop_desc, IID_PPV_ARGS(&queue));
    *out = queue;
    return result;
}
int32_t create_completion_fence(void* device, void** out) {
    ID3D12Fence* fence = nullptr;
    const auto result = static_cast<ID3D12Device*>(device)->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence));
    *out = fence;
    return result;
}
void* create_event(uint32_t initially_signaled) {
    return CreateEventW(nullptr, FALSE, initially_signaled != 0, nullptr);
}
void* duplicate_event(void* event) {
    HANDLE duplicate = nullptr;
    DuplicateHandle(GetCurrentProcess(), event, GetCurrentProcess(), &duplicate, 0, FALSE, DUPLICATE_SAME_ACCESS);
    return duplicate;
}
void close_event(void* event) { CloseHandle(event); }
int32_t create_upload_fence(void* b) {
    auto& self = *as_bridge(b);
    return rsf_shared_fence_create(self.device.Get(), self.graphics.native_device, settings().log,
        settings().user, &self.shared_fence) == RSF_SHARED_OK ? S_OK : E_FAIL;
}
// Slot lists record on the graphics device: Streamline's proxy device when a host exists.
int32_t create_slot(void* b, void** allocator, void** list) {
    auto* gpu = static_cast<ID3D12Device*>(as_bridge(b)->graphics.device);
    ID3D12CommandAllocator* created_allocator = nullptr;
    ID3D12GraphicsCommandList* created_list = nullptr;
    *allocator = *list = nullptr;
    if (!rsf::create_command_list(gpu, D3D12_COMMAND_LIST_TYPE_DIRECT, &created_allocator, &created_list))
        return E_FAIL;
    *allocator = created_allocator; *list = created_list;
    return S_OK;
}
int32_t create_render_buffer(void* b, uint32_t width, uint32_t height, uint32_t format, void** out) {
    D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = width; desc.Height = height; desc.Format = DXGI_FORMAT(format);
    desc.DepthOrArraySize = desc.MipLevels = 1;
    desc.SampleDesc.Count = 1; desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    ID3D12Resource* value = nullptr;
    const auto hr = as_bridge(b)->direct_device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE,
        &desc, D3D12_RESOURCE_STATE_PRESENT, nullptr, IID_PPV_ARGS(&value));
    *out = value;
    return hr;
}

int32_t create_plain(void* b, const rsf_d3d11p_plain_plan* plan, void** physical, void** queue) {
    auto& self = *as_bridge(b);
    auto desc = self.description;
    desc.SwapEffect = static_cast<DXGI_SWAP_EFFECT>(plan->swap_effect);
    desc.Flags = plan->flags;
    if (plan->buffer_count) desc.BufferCount = plan->buffer_count;
    if (plan->buffer_usage) desc.BufferUsage = plan->buffer_usage;
    ComPtr<IDXGISwapChain> plain;
    auto* owner_queue = static_cast<ID3D12CommandQueue*>(plan->queue);
    const HRESULT created = original_create ? original_create(self.parent.Get(), owner_queue, &desc, &plain) :
        self.parent->CreateSwapChain(owner_queue, &desc, &plain);
    if (FAILED(created)) {
        char text[512]{}; std::snprintf(text, sizeof(text),
            "Physical chain creation refused HRESULT=0x%08lx facade=%p factory=%p queue=%p window=%p size=%ux%u format=%u buffers=%u effect=%u flags=0x%x native12=%u usage=0x%x sample=%u/%u refresh=%u/%u scanline=%u scaling=%u windowed=%u",
            static_cast<unsigned long>(created), b, static_cast<void*>(self.parent.Get()),
            static_cast<void*>(owner_queue), static_cast<void*>(desc.OutputWindow), desc.BufferDesc.Width, desc.BufferDesc.Height,
            uint32_t(desc.BufferDesc.Format), desc.BufferCount, uint32_t(desc.SwapEffect), desc.Flags, uint32_t(self.native12),
            desc.BufferUsage,desc.SampleDesc.Count,desc.SampleDesc.Quality,
            desc.BufferDesc.RefreshRate.Numerator,desc.BufferDesc.RefreshRate.Denominator,
            uint32_t(desc.BufferDesc.ScanlineOrdering),uint32_t(desc.BufferDesc.Scaling),uint32_t(desc.Windowed!=FALSE));
        log(text); return E_FAIL;
    }
    ComPtr<IDXGISwapChain4> replacement;
    if (FAILED(plain.As(&replacement))) return E_FAIL;
    *physical = replacement.Detach();
    *queue = owner_queue;
    return S_OK;
}
int32_t query_chain4(void* chain, void** physical) {
    IDXGISwapChain4* replacement = nullptr;
    const auto result = static_cast<IUnknown*>(chain)->QueryInterface(IID_PPV_ARGS(&replacement));
    *physical = replacement;
    return result;
}
int32_t get_fullscreen(void* physical, int32_t* fullscreen) {
    auto* chain = static_cast<IDXGISwapChain4*>(physical);
    BOOL value = FALSE; const auto result = chain->GetFullscreenState(&value, nullptr); *fullscreen = value; return result;
}
int32_t set_fullscreen(void* physical, int32_t fullscreen, void* target) {
    return static_cast<IDXGISwapChain4*>(physical)->SetFullscreenState(BOOL(fullscreen), static_cast<IDXGIOutput*>(target));
}
int32_t physical_present(void* physical, uint32_t sync, uint32_t flags) {
    return static_cast<IDXGISwapChain4*>(physical)->Present(sync, flags);
}
int32_t resize_physical(void* b, void* physical, uint32_t count, uint32_t width, uint32_t height, uint32_t format) {
    auto& self = *as_bridge(b);
    return static_cast<IDXGISwapChain4*>(physical)->ResizeBuffers(count, width, height, DXGI_FORMAT(format),
        self.tearing_supported ? DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING : 0);
}

int32_t join(void* signaller, void* waiter, void* fence, uint64_t value) {
    auto* complete = static_cast<ID3D12Fence*>(fence);
    const auto signaled = static_cast<ID3D12CommandQueue*>(signaller)->Signal(complete, value);
    return FAILED(signaled) ? signaled : static_cast<ID3D12CommandQueue*>(waiter)->Wait(complete, value);
}
int32_t wait_fence(void* fence, void* event, uint64_t value) {
    return rsf::wait_fence(static_cast<ID3D12Fence*>(fence), value, event);
}
int32_t slot_reset(void* fence, void* event, void* raw_allocator, void* raw_list, uint64_t value, uint32_t* step) {
    auto* allocator = static_cast<ID3D12CommandAllocator*>(raw_allocator);
    auto* list = static_cast<ID3D12GraphicsCommandList*>(raw_list);
    *step = 0; HRESULT result = wait_fence(fence, event, value); if (FAILED(result)) return result;
    *step = 1; result = allocator->Reset(); if (FAILED(result)) return result;
    *step = 2; return list->Reset(allocator, nullptr);
}
// Signal with the preceding D3D11 work submitted before D3D12 waits on it.
// Flushing before Signal leaves the dependency buffered until a later frame.
int32_t upload_wait(void* b, void* queue, uint64_t value) {
    auto& self = *as_bridge(b);
    auto* fence12 = static_cast<ID3D12Fence*>(rsf_shared_fence_d3d12(self.shared_fence));
    auto* fence11 = static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(self.shared_fence));
    const auto signaled = self.context->Signal(fence11, value);
    if (FAILED(signaled)) return signaled;
    self.context->Flush();
    return static_cast<ID3D12CommandQueue*>(queue)->Wait(fence12, value);
}
int32_t acquire_backbuffer(void* chain, void** out) {
    auto* physical = static_cast<IDXGISwapChain4*>(chain);
    ID3D12Resource* value = nullptr;
    const auto hr = physical->GetBuffer(physical->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&value));
    *out = value;
    return hr;
}
void record_copy(void* b, void* raw_list, void* backbuffer, void* source) {
    auto& self = *as_bridge(b); auto* list = static_cast<ID3D12GraphicsCommandList*>(raw_list);
    const auto source_state = self.native12 ? D3D12_RESOURCE_STATE_PRESENT : D3D12_RESOURCE_STATE_COMMON;
    rsf::copy_transitioned(list, static_cast<ID3D12Resource*>(backbuffer),
        D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_PRESENT,
        static_cast<ID3D12Resource*>(source), source_state, source_state);
}
void* context11(void* b) { return as_bridge(b)->context.Get(); }
int32_t close_execute(void* queue, void* list, void* command) {
    const auto closed = static_cast<ID3D12GraphicsCommandList*>(list)->Close();
    if (SUCCEEDED(closed)) {
        ID3D12CommandList* lists[]{static_cast<ID3D12GraphicsCommandList*>(command)};
        static_cast<ID3D12CommandQueue*>(queue)->ExecuteCommandLists(1, lists);
    }
    return closed;
}
int32_t upload_return(void* b, void* raw_queue, uint64_t value) {
    auto& self = *as_bridge(b); auto* queue = static_cast<ID3D12CommandQueue*>(raw_queue);
    auto* fence12 = static_cast<ID3D12Fence*>(rsf_shared_fence_d3d12(self.shared_fence));
    auto* fence11 = static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(self.shared_fence));
    HRESULT result = queue->Signal(fence12, value);
    if (SUCCEEDED(result)) result = queue->Wait(fence12, value);
    if (SUCCEEDED(result)) result = self.context->Wait(fence11, value);
    return result;
}
int32_t signal_slot(void* queue, void* fence, uint64_t value) {
    return static_cast<ID3D12CommandQueue*>(queue)->Signal(static_cast<ID3D12Fence*>(fence), value);
}
int32_t application_ready(void* event, void* fence, uint64_t value) {
    if (fence) return static_cast<ID3D12Fence*>(fence)->SetEventOnCompletion(value, event);
    return SetEvent(event) ? S_OK : E_FAIL;
}
void report_device_failure(void* b, int32_t result, uint32_t backend) {
    auto& self = *as_bridge(b);
    char text[160]; std::snprintf(text, sizeof(text), "Native D3D12 presentation failed backend=%u HRESULT=%08lx device_reason=%08lx",
        backend, static_cast<unsigned long>(result), static_cast<unsigned long>(self.direct_device->GetDeviceRemovedReason())); log(text);
    ComPtr<ID3D12InfoQueue> messages;
    if (FAILED(self.direct_device.As(&messages))) return;
    unsigned reported = 0;
    const auto count = messages->GetNumStoredMessagesAllowedByRetrievalFilter();
    for (UINT64 i = count; i && reported < 8;) {
        --i;
        SIZE_T bytes = 0; if (FAILED(messages->GetMessage(i, nullptr, &bytes))) continue;
        auto storage = std::make_unique<unsigned char[]>(bytes);
        auto* message = reinterpret_cast<D3D12_MESSAGE*>(storage.get());
        if (SUCCEEDED(messages->GetMessage(i, message, &bytes)) && message->Severity <= D3D12_MESSAGE_SEVERITY_ERROR) {
            log(message->pDescription); ++reported;
        }
    }
}
void log_present_failure(void* b, const char* step, int32_t result, uint32_t render_index) {
    const auto& d = as_bridge(b)->description;
    char text[192]; std::snprintf(text, sizeof(text), "Native D3D12 presentation step '%s' failed HRESULT=%08lx render_index=%u buffers=%u size=%ux%u",
        step, static_cast<unsigned long>(result), render_index, d.BufferCount, d.BufferDesc.Width, d.BufferDesc.Height);
    log(text);
}

int32_t query_buffer_interface(void* resource, const void* iid, void** out) {
    return static_cast<IUnknown*>(resource)->QueryInterface(*static_cast<const IID*>(iid), out);
}
int32_t get_physical_buffer(void* physical, uint32_t index, const void* iid, void** out) {
    return static_cast<IDXGISwapChain4*>(physical)->GetBuffer(index, *static_cast<const IID*>(iid), out);
}
void log_buffer_refusal(void* b, uint32_t index, const void* requested, int32_t result, uint32_t pending) {
    const auto& iid = *static_cast<const IID*>(requested);
    char text[320]{};
    std::snprintf(text, sizeof(text),
        "GetBuffer refused HRESULT=0x%08lx facade=%p index=%u IID=%08lx-%04x-%04x-%02x%02x-%02x%02x%02x%02x%02x%02x output=%p native12=%u physical_pending=%u",
        static_cast<unsigned long>(result), b, index, static_cast<unsigned long>(iid.Data1),
        uint32_t(iid.Data2), uint32_t(iid.Data3), uint32_t(iid.Data4[0]), uint32_t(iid.Data4[1]),
        uint32_t(iid.Data4[2]), uint32_t(iid.Data4[3]), uint32_t(iid.Data4[4]), uint32_t(iid.Data4[5]),
        uint32_t(iid.Data4[6]), uint32_t(iid.Data4[7]), static_cast<void*>(nullptr), uint32_t(as_bridge(b)->native12), pending);
    log(text);
}

int32_t signal_wait(void* queue, void* fence, void* event, uint64_t value) {
    if (FAILED(signal_slot(queue, fence, value))) return E_FAIL;
    return wait_fence(fence, event, value);
}
uint32_t buffer_referenced(void* resource) {
    auto* buffer = static_cast<IUnknown*>(resource);
    buffer->AddRef();
    return buffer->Release() > 1 ? 1u : 0u;
}
void set_description(void* b, uint32_t count, uint32_t width, uint32_t height, uint32_t format, uint32_t flags) {
    auto& d = as_bridge(b)->description;
    d.BufferCount = count;
    d.BufferDesc.Width = width; d.BufferDesc.Height = height; d.BufferDesc.Format = DXGI_FORMAT(format); d.Flags = flags;
}
int32_t create_retirement_fence(void* b, void** out) {
    return create_completion_fence(as_bridge(b)->graphics.native_device, out);
}
int32_t retire_submit(void* b, void* raw_queue, void* retirement_fence, void* vendor_fence, uint64_t vendor_value, void* list, uint64_t value) {
    auto& self = *as_bridge(b); auto* queue = static_cast<ID3D12CommandQueue*>(raw_queue);
    if (vendor_fence && FAILED(queue->Wait(static_cast<ID3D12Fence*>(vendor_fence), vendor_value)))
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    if (list) {
        auto* command = static_cast<ID3D12GraphicsCommandList*>(self.provider_list(queue, static_cast<ID3D12GraphicsCommandList*>(list)));
        if (!command) return RSF_BACKEND_ERROR_FEATURE_FAILED;
        ID3D12CommandList* lists[]{command}; queue->ExecuteCommandLists(1, lists);
    }
    return FAILED(signal_slot(queue, retirement_fence, value)) ? RSF_BACKEND_ERROR_FEATURE_FAILED : RSF_BACKEND_OK;
}
// On device failure retain GPU-reachable storage for the process rather than free it.
// The Rust owner has already retained its synchronization resources and slots.
void retain_all(void* b) {
    auto& self = *as_bridge(b);
    self.context.Detach(); self.device.Detach(); self.parent.Detach();
    self.direct_device.Detach(); self.direct_queue.Detach();
}
void release_presentation(void* b) {
    auto& self = *as_bridge(b);
    rsf_shared_fence_destroy(self.shared_fence); self.shared_fence = nullptr;
}
void release_direct_graphics(void* b) {
    auto& self = *as_bridge(b);
    self.direct_queue.Reset(); self.direct_device.Reset();
}
// A private host ends after the Rust owner has released the slot lists on its device.
void release_host(void* b) {
    auto& self = *as_bridge(b);
    self.context.Reset(); self.device.Reset(); self.parent.Reset();
    rsf_streamline_host_destroy(self.host);
}
void* source_queue(void* b, void* interop_queue) {
    auto& self = *as_bridge(b);
    auto* queue = static_cast<ID3D12CommandQueue*>(interop_queue);
    if (!self.native12 && self.host && queue)
        queue = static_cast<ID3D12CommandQueue*>(rsf_streamline_host_native(self.host, queue));
    if (queue) queue->AddRef();
    return queue;
}
void* command_list(void* b, void* queue, void* list) {
    return as_bridge(b)->provider_list(queue, static_cast<ID3D12GraphicsCommandList*>(list));
}
int32_t verify_outer(void* inner, void* window, void* device, void* context, void* outer) {
    auto* captured = static_cast<IDXGISwapChain*>(inner);
    auto* chain = static_cast<IDXGISwapChain*>(outer);
    ComPtr<ID3D11Device> captured_device, context_device, outer_device;
    DXGI_SWAP_CHAIN_DESC desc{};
    if (!same_identity(chain, captured) || FAILED(chain->GetDesc(&desc)) || desc.OutputWindow != window ||
        FAILED(captured->GetDevice(IID_PPV_ARGS(&captured_device))) || FAILED(chain->GetDevice(IID_PPV_ARGS(&outer_device)))) return E_INVALIDARG;
    auto* immediate = static_cast<ID3D11DeviceContext*>(context);
    if (immediate->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return E_INVALIDARG;
    immediate->GetDevice(&context_device);
    if (!same_identity(captured_device.Get(), static_cast<IUnknown*>(device)) ||
        !same_identity(captured_device.Get(), outer_device.Get()) || !same_identity(captured_device.Get(), context_device.Get())) return E_INVALIDARG;
    return S_OK;
}
} // namespace leaf

const rsf_d3d11p_platform platform{
    .provider_table = leaf::provider_table, .observer_present = leaf::observer_present,
    .window_valid = leaf::window_valid, .client_size = leaf::client_size, .canonical_identity = leaf::canonical_identity,
    .add_ref = leaf::add_ref, .release = leaf::release,
    .host_sleep = leaf::host_sleep, .host_marker = leaf::host_marker, .host_acquire = leaf::host_acquire,
    .host_input = leaf::host_input, .host_abort = leaf::host_abort, .host_reflex_off = leaf::host_reflex_off,
    .host_generation_load = leaf::host_generation_load, .host_present_common = leaf::host_present_common,
    .bridge_new = leaf::bridge_new, .bridge_release_owned = leaf::bridge_release_owned,
    .bridge_chain = leaf::bridge_chain, .facts = leaf::facts,
    .init_source = leaf::init_source, .owning_adapter = leaf::owning_adapter, .native_device = leaf::native_device,
    .adopt_host = leaf::adopt_host, .d3d11_device = leaf::d3d11_device, .create_host = leaf::create_host,
    .host = leaf::host, .resolve_graphics = leaf::resolve_graphics, .tearing_supported = leaf::tearing_supported,
    .fsr4_supported = leaf::fsr4_supported, .create_interop_queue = leaf::create_interop_queue,
    .create_completion_fence = leaf::create_completion_fence, .create_event = leaf::create_event,
    .duplicate_event = leaf::duplicate_event, .close_event = leaf::close_event,
    .create_upload_fence = leaf::create_upload_fence, .create_slot = leaf::create_slot,
    .shared_surface_platform = rsf_shared_surface_native_platform, .create_render_buffer = leaf::create_render_buffer,
    .create_plain = leaf::create_plain, .query_chain4 = leaf::query_chain4, .get_fullscreen = leaf::get_fullscreen,
    .set_fullscreen = leaf::set_fullscreen,
    .physical_present = leaf::physical_present, .resize_physical = leaf::resize_physical,
    .join = leaf::join,
    .slot_reset = leaf::slot_reset, .upload_wait = leaf::upload_wait, .acquire_backbuffer = leaf::acquire_backbuffer,
    .record_copy = leaf::record_copy, .context11 = leaf::context11,
    .close_execute = leaf::close_execute, .upload_return = leaf::upload_return, .signal_slot = leaf::signal_slot,
    .application_ready = leaf::application_ready, .report_device_failure = leaf::report_device_failure,
    .log_present_failure = leaf::log_present_failure,
    .query_buffer_interface = leaf::query_buffer_interface, .get_physical_buffer = leaf::get_physical_buffer,
    .log_buffer_refusal = leaf::log_buffer_refusal,
    .signal_wait = leaf::signal_wait, .wait_fence = leaf::wait_fence,
    .buffer_referenced = leaf::buffer_referenced, .set_description = leaf::set_description,
    .create_retirement_fence = leaf::create_retirement_fence, .retire_submit = leaf::retire_submit, .retain_all = leaf::retain_all,
    .release_presentation = leaf::release_presentation,
    .release_direct_graphics = leaf::release_direct_graphics,
    .release_host = leaf::release_host,
    .source_queue = leaf::source_queue,
    .command_list = leaf::command_list, .verify_outer = leaf::verify_outer,
};

rsf_d3d11p_request normalize(const DXGI_SWAP_CHAIN_DESC& desc) {
    return {desc.OutputWindow, desc.BufferDesc.Width, desc.BufferDesc.Height, uint32_t(desc.BufferDesc.Format),
        desc.SampleDesc.Count, desc.BufferCount, desc.Windowed ? 1u : 0u, desc.Flags, uint32_t(desc.SwapEffect),
        desc.BufferDesc.RefreshRate.Numerator, desc.BufferDesc.RefreshRate.Denominator};
}
HRESULT create_bridge(IDXGIFactory* factory, IUnknown* unknown, const DXGI_SWAP_CHAIN_DESC& desc, IDXGISwapChain** out) {
    const auto request = normalize(desc);
    return rsf_core_d3d11p_create(&platform, factory, unknown, &request, &desc, reinterpret_cast<void**>(out));
}
HRESULT STDMETHODCALLTYPE create_hook(IDXGIFactory* factory, IUnknown* device, DXGI_SWAP_CHAIN_DESC* desc, IDXGISwapChain** out) {
    if (desc && rsf_core_d3d11p_capture(desc->OutputWindow) && SUCCEEDED(create_bridge(factory, device, *desc, out))) {
        rsf_core_d3d11p_captured(&platform, *out);
        return S_OK;
    }
    return original_create(factory, device, desc, out);
}
HRESULT STDMETHODCALLTYPE hwnd_hook(IDXGIFactory2* factory, IUnknown* device, HWND hwnd,
    const DXGI_SWAP_CHAIN_DESC1* desc, const DXGI_SWAP_CHAIN_FULLSCREEN_DESC* fullscreen, IDXGIOutput* output, IDXGISwapChain1** out) {
    if (desc && !output && !desc->Stereo && rsf_core_d3d11p_capture(hwnd)) {
        // Keep the application's fullscreen mode. Dropping its refresh rate made GetDesc report
        // 0/0, which RDR2 matched to its lowest listed mode (30 Hz on a 30-120 Hz panel).
        DXGI_SWAP_CHAIN_DESC translated{}; translated.BufferDesc = {desc->Width, desc->Height,
            fullscreen ? fullscreen->RefreshRate : DXGI_RATIONAL{}, desc->Format,
            fullscreen ? fullscreen->ScanlineOrdering : DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
            fullscreen ? fullscreen->Scaling : DXGI_MODE_SCALING_UNSPECIFIED};
        translated.SampleDesc = desc->SampleDesc; translated.BufferUsage = desc->BufferUsage; translated.BufferCount = desc->BufferCount;
        translated.OutputWindow = hwnd; translated.Windowed = fullscreen ? fullscreen->Windowed : TRUE; translated.SwapEffect = desc->SwapEffect; translated.Flags = desc->Flags;
        ComPtr<IDXGISwapChain> bridge;
        if (SUCCEEDED(create_bridge(factory, device, translated, &bridge))) {
            const auto result = bridge->QueryInterface(IID_PPV_ARGS(out));
            if (SUCCEEDED(result)) rsf_core_d3d11p_captured(&platform, bridge.Get());
            return result;
        }
    }
    return original_hwnd(factory, device, hwnd, desc, fullscreen, output, out);
}
}
extern "C" const rsf_generation_provider* rsf_d3d11p_provider_for(uint32_t backend) {
    return leaf::provider_table(rsf_core_fg_provider_family(backend));
}
extern "C" int rsf_d3d11_present_prepare(const rsf_d3d11_present_setup* setup) {
    return rsf_core_d3d11p_prepare(setup, original_create != nullptr);
}
extern "C" int32_t rsf_d3d11_present_create_for_hwnd(void* factory, void* queue, void* hwnd,
    const void* description, void** out) try {
    if (!factory || !queue || !description || !out) return E_INVALIDARG;
    *out = nullptr;
    const auto& desc = *static_cast<const DXGI_SWAP_CHAIN_DESC1*>(description);
    if (desc.Stereo) return DXGI_ERROR_UNSUPPORTED;
    DXGI_SWAP_CHAIN_DESC translated{};
    translated.BufferDesc = {desc.Width, desc.Height, {}, desc.Format, DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED, DXGI_MODE_SCALING_UNSPECIFIED};
    translated.SampleDesc = desc.SampleDesc; translated.BufferUsage = desc.BufferUsage; translated.BufferCount = desc.BufferCount;
    translated.OutputWindow = static_cast<HWND>(hwnd); translated.Windowed = TRUE;
    translated.SwapEffect = desc.SwapEffect; translated.Flags = desc.Flags;
    ComPtr<IDXGISwapChain> bridge;
    const HRESULT result = create_bridge(static_cast<IDXGIFactory*>(factory), static_cast<IUnknown*>(queue), translated, &bridge);
    return SUCCEEDED(result) ? bridge->QueryInterface(__uuidof(IDXGISwapChain1), out) : result;
} catch (...) {
    if (out) *out = nullptr;
    return E_FAIL;
}
extern "C" int32_t rsf_d3d11_present_wrap_native_chain(void* raw_device, void* raw_chain, void** out) try {
    if (!out) return E_INVALIDARG;
    *out = nullptr;
    if (!raw_device || !raw_chain || !settings().accept_window || original_create || rsf_core_d3d11p_active_bridge()) return E_INVALIDARG;
    auto* device = static_cast<ID3D11Device*>(raw_device);
    auto* native_chain = static_cast<IDXGISwapChain*>(raw_chain);
    DXGI_SWAP_CHAIN_DESC description{};
    ComPtr<ID3D11Device> chain_device;
    ComPtr<IUnknown> expected, observed;
    ComPtr<IDXGIFactory> factory;
    if (FAILED(native_chain->GetDesc(&description)) || FAILED(native_chain->GetDevice(IID_PPV_ARGS(&chain_device))) ||
        FAILED(device->QueryInterface(IID_PPV_ARGS(&expected))) || FAILED(chain_device.As(&observed)) ||
        expected.Get() != observed.Get() || FAILED(native_chain->GetParent(IID_PPV_ARGS(&factory)))) return E_INVALIDARG;
    ComPtr<IDXGISwapChain> facade;
    const auto result = create_bridge(factory.Get(), device, description, &facade);
    if (FAILED(result)) return result;
    if (!rsf_d3d11_present_is_owner(facade.Get())) return E_UNEXPECTED;
    *out = facade.Detach(); return S_OK;
} catch (...) { return E_FAIL; }
extern "C" int rsf_d3d11_present_install(const rsf_d3d11_present_setup* setup) {
    if (!rsf_d3d11_present_prepare(setup)) return 0;
    ComPtr<IDXGIFactory2> factory;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return 0;
    const auto initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return 0;
    auto** table = *reinterpret_cast<void***>(factory.Get());
    if (MH_CreateHook(table[10], reinterpret_cast<void*>(&create_hook), reinterpret_cast<void**>(&original_create)) != MH_OK) return 0;
    if (MH_CreateHook(table[15], reinterpret_cast<void*>(&hwnd_hook), reinterpret_cast<void**>(&original_hwnd)) != MH_OK) { MH_RemoveHook(table[10]); original_create = nullptr; return 0; }
    if (MH_EnableHook(table[10]) != MH_OK || MH_EnableHook(table[15]) != MH_OK) { MH_DisableHook(table[10]); MH_RemoveHook(table[10]); MH_RemoveHook(table[15]); original_create = nullptr; return 0; }
    log("presentation bridge: cold-start D3D11 swapchain interception installed"); return 1;
}
extern "C" int rsf_d3d11_present_install_scoped(const rsf_d3d11_present_setup* setup) {
    rsf_core_d3d11p_set_scoped(1);
    if (!rsf_d3d11_present_install(setup)) { rsf_core_d3d11p_set_scoped(0); return 0; }
    return 1;
}
extern "C" int32_t rsf_d3d11_present_creation_begin(void* window, uint64_t* token) {
    return rsf_core_d3d11p_creation_begin(&platform, window, token, original_create != nullptr);
}
extern "C" int32_t rsf_d3d11_present_creation_end(void* window, uint64_t token, void* device, void* context, void* outer) {
    return rsf_core_d3d11p_creation_end(&platform, window, token, device, context, outer);
}
extern "C" rsf_streamline_host* rsf_d3d11_present_host() { return static_cast<rsf_streamline_host*>(rsf_core_d3d11p_host()); }
extern "C" int rsf_d3d11_present_graphics(rsf_streamline_graphics* graphics) { return rsf_core_d3d11p_graphics(graphics); }
extern "C" void* rsf_d3d11_present_queue() { return rsf_core_d3d11p_queue(&platform); }
extern "C" void* rsf_d3d11_present_source_queue() { return rsf_core_d3d11p_source_queue(&platform); }
extern "C" int rsf_d3d11_present_is_owner(void* swapchain) { return rsf_core_d3d11p_is_owner(&platform, swapchain); }
extern "C" int rsf_d3d11_present_dispatch_owned(void* receiver, uint32_t sync, uint32_t flags, int32_t* result) {
    if (!result) return 0;
    rsf_d3d11_chain_dispatch request{}; request.receiver = receiver; request.method = RSF_D3D11_DISPATCH_PRESENT;
    request.args.present = {sync,flags};
    rsf_d3d11_chain_dispatch_owned(&request);
    if (request.handled) *result = request.result.hresult;
    return int(request.handled);
}
extern "C" void rsf_d3d11_chain_dispatch_owned(rsf_d3d11_chain_dispatch* request) {
    if (!request) return;
    const auto admitted = rsf_core_d3d11p_dispatch_admit(request->receiver);
    if (!admitted) return;
    auto* bridge = as_bridge(request->receiver);
    request->handled = 1;
    if (request->method == RSF_D3D11_DISPATCH_RELEASE) {
        // Consumes exactly the caller's reference. The object may be gone on return.
        request->result.references = bridge->release_owned(); return;
    }
    // The caller's live COM lease makes this AddRef safe, including during shutdown.
    // Keep that exact owner alive across all callbacks without holding a host lock.
    ComPtr<IDXGISwapChain4> lease = bridge;
    request->result.hresult = E_FAIL;
    try {
        if (admitted == 2) { request->result.hresult = DXGI_ERROR_INVALID_CALL; return; }
        const auto& args = request->args;
        switch (request->method) {
        case RSF_D3D11_DISPATCH_PRESENT:
            request->result.hresult = bridge->present_owned(args.present.sync_interval,args.present.flags); break;
        case RSF_D3D11_DISPATCH_PRESENT1:
            request->result.hresult = bridge->present1_owned(args.present1.sync_interval,args.present1.flags,
                static_cast<const DXGI_PRESENT_PARAMETERS*>(args.present1.parameters)); break;
        case RSF_D3D11_DISPATCH_SET_FULLSCREEN_STATE:
            request->result.hresult = bridge->set_fullscreen_owned(BOOL(args.fullscreen.fullscreen),
                static_cast<IDXGIOutput*>(args.fullscreen.target)); break;
        case RSF_D3D11_DISPATCH_RESIZE_BUFFERS:
            request->result.hresult = bridge->resize_buffers_owned(args.resize_buffers.buffer_count,
                args.resize_buffers.width,args.resize_buffers.height,DXGI_FORMAT(args.resize_buffers.format),args.resize_buffers.flags); break;
        case RSF_D3D11_DISPATCH_SET_COLOR_SPACE1:
            request->result.hresult = bridge->set_color_space_owned(DXGI_COLOR_SPACE_TYPE(args.color_space.color_space)); break;
        case RSF_D3D11_DISPATCH_RESIZE_BUFFERS1:
            request->result.hresult = bridge->resize_buffers1_owned(args.resize_buffers1.buffer_count,
                args.resize_buffers1.width,args.resize_buffers1.height,DXGI_FORMAT(args.resize_buffers1.format),
                args.resize_buffers1.flags,args.resize_buffers1.node_masks,
                reinterpret_cast<IUnknown* const*>(args.resize_buffers1.present_queues)); break;
        }
    } catch (...) { /* A known facade must never fall through to a native receiver. */ }
}
extern "C" int rsf_d3d11_present_has_owner() { return rsf_core_d3d11p_has_owner(); }
extern "C" const rsf_generation_provider* rsf_d3d11_present_provider() { return rsf_core_d3d11p_provider(&platform); }
extern "C" int32_t rsf_d3d11_present_status_get(rsf_d3d11_present_status* status) { return rsf_core_d3d11p_status_get(status); }
extern "C" void* rsf_d3d11_present_session() { return rsf_core_d3d11p_session(); }
extern "C" uint32_t rsf_d3d11_present_backend() { return rsf_core_d3d11p_backend(); }
extern "C" uint32_t rsf_d3d11_present_backend_choices() { return rsf_core_d3d11p_backend_choices(); }
extern "C" int32_t rsf_d3d11_present_request(uint32_t backend) { return rsf_core_d3d11p_request(backend); }
extern "C" int32_t rsf_d3d11_present_switch_result() { return rsf_core_d3d11p_switch_result(); }
extern "C" uint64_t rsf_d3d11_present_generation() { return rsf_core_d3d11p_generation(); }
extern "C" int32_t rsf_d3d11_present_retire(void* session, void* list, void** fence, uint64_t* value) {
    return rsf_core_d3d11p_retire(&platform, session, list, fence, value);
}
extern "C" void* rsf_d3d11_present_command_list(void* list) { return rsf_core_d3d11p_command_list(&platform, list); }
extern "C" int32_t rsf_d3d11_present_begin(uint64_t id, uint64_t* generation) { return rsf_core_d3d11p_begin(&platform, id, generation); }
extern "C" int32_t rsf_d3d11_present_pace(uint64_t id, rsf_pacing_owner owner, uint64_t* generation) {
    return rsf_core_d3d11p_pace(&platform, id, owner, nullptr, generation);
}
extern "C" int32_t rsf_d3d11_present_pace_for_generation(uint64_t id, rsf_pacing_owner owner, uint64_t generation) {
    return rsf_core_d3d11p_pace(&platform, id, owner, &generation, nullptr);
}
extern "C" int32_t rsf_d3d11_present_marker(uint64_t generation, uint64_t id, rsf_latency_marker marker, uint32_t controller) {
    return rsf_core_d3d11p_marker(&platform, generation, id, marker, controller);
}
extern "C" int32_t rsf_d3d11_present_acquire(uint64_t id) { return rsf_core_d3d11p_acquire(&platform, id); }
extern "C" int32_t rsf_d3d11_present_receipt(uint64_t generation,uint64_t id,uint32_t kinds,uint32_t message) {
    return rsf_core_d3d11p_receipt(&platform, generation, id, kinds, message);
}
extern "C" int32_t rsf_d3d11_present_input(uint64_t id, uint32_t kinds, uint32_t message) {
    return rsf_core_d3d11p_input(&platform, id, kinds, message);
}
extern "C" int32_t rsf_d3d11_present_abort(uint64_t id) { return rsf_core_d3d11p_abort(&platform, id); }
