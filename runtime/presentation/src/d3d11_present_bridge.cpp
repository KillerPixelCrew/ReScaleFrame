// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/d3d11_present_bridge.h>
#include <rescaleframe/shared_surface.h>
#include "../../backends/common/d3d12_helpers.h"
#include <d3d11_4.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <atomic>
#include <mutex>
#include <shared_mutex>
#include <string>
#include <new>
#include <cstdio>
#include <array>
#include <algorithm>
#include <memory>

namespace {
using Microsoft::WRL::ComPtr;
rsf_d3d11_present_setup settings{};
std::string runtime_directory;
std::string fsr3_directory, fsr4_directory, xess_directory;
std::string streamline_directory;
std::string engine_version, project_id;
std::mutex creation_guard;
std::shared_mutex provider_guard;
std::atomic<uint32_t> active_backend{RSF_FG_BACKEND_DLSS}, requested_backend{UINT32_MAX};
std::atomic<int32_t> switch_result{0};
std::atomic<uint64_t> chain_generation{1};
std::atomic<rsf_streamline_host*> active_host{nullptr};
std::atomic<void*> active_chain{nullptr};
std::atomic<ID3D12CommandQueue*> active_interop{nullptr};
std::atomic<void*> active_session{nullptr};
// The active provider paces through the shared Streamline host (Reflex), so latency calls go to
// the host once rather than to both. Changes only with active_session, under provider_guard.
std::atomic<bool> host_paced{false};
rsf_streamline_graphics active_graphics{};
// Same mapping as the orchestrator's rsf_fg_get_provider, which this library cannot call: the
// orchestrator links presentation, not the reverse. Unknown ids, and Off, have no provider.
const rsf_generation_provider* backend_api(uint32_t backend) {
    switch (backend) {
    case RSF_FG_BACKEND_DLSS: return rsf_generation_dlss();
    case RSF_FG_BACKEND_FSR3:
    case RSF_FG_BACKEND_FSR4: return rsf_generation_fsr();
    case RSF_FG_BACKEND_XESS: return rsf_generation_xess();
    default: return nullptr;
    }
}
const rsf_generation_provider* generation_api() { return backend_api(active_backend.load()); }
const std::string& sdk_directory(uint32_t backend) {
    switch (backend) {
    case RSF_FG_BACKEND_DLSS: return streamline_directory;
    case RSF_FG_BACKEND_FSR4: return fsr4_directory;
    case RSF_FG_BACKEND_XESS: return xess_directory;
    default: return fsr3_directory;
    }
}
constexpr rsf_fg_options fg_off{sizeof(rsf_fg_options), RSF_FG_ABI_VERSION, RSF_FG_OFF, 1, 0, RSF_REFLEX_OFF, 0};
using Create = HRESULT(STDMETHODCALLTYPE*)(IDXGIFactory*, IUnknown*, DXGI_SWAP_CHAIN_DESC*, IDXGISwapChain**);
using CreateHwnd = HRESULT(STDMETHODCALLTYPE*)(IDXGIFactory2*, IUnknown*, HWND,
    const DXGI_SWAP_CHAIN_DESC1*, const DXGI_SWAP_CHAIN_FULLSCREEN_DESC*, IDXGIOutput*, IDXGISwapChain1**);
Create original_create = nullptr;
CreateHwnd original_hwnd = nullptr;
thread_local bool creating = false;

void log(const char* message) { if (settings.log) settings.log(settings.user, message); }
uint64_t clock_tick() { LARGE_INTEGER now{}; QueryPerformanceCounter(&now); return uint64_t(now.QuadPart); }

class Bridge final : public IDXGISwapChain4 {
    std::atomic<ULONG> references{1};
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext4> context;
    ComPtr<IDXGIFactory> parent;
    ComPtr<IDXGISwapChain4> physical;
    std::mutex physical_guard;
    std::atomic<UINT> application_presents{0};
    template<class Call> HRESULT query_physical(Call call) {
        std::lock_guard<std::mutex> lock(physical_guard);
        return physical ? call(physical.Get()) : DXGI_ERROR_WAS_STILL_DRAWING;
    }
    // Calls that make DXGI send window messages run outside the lock: a window thread that
    // queries the chain while handling them would otherwise wait for this one.
    ComPtr<IDXGISwapChain4> current_physical() {
        std::lock_guard<std::mutex> lock(physical_guard); return physical;
    }
    void clear_physical() {
        std::lock_guard<std::mutex> lock(physical_guard); physical.Reset(); provider_queue.store(nullptr);
    }
    void publish_physical(ComPtr<IDXGISwapChain4>& value, ID3D12CommandQueue* queue) {
        std::lock_guard<std::mutex> lock(physical_guard); physical = std::move(value); provider_queue.store(queue);
    }
    // The queue the current physical chain (and its provider) was created on.
    std::atomic<ID3D12CommandQueue*> provider_queue{nullptr};
    bool tearing_supported = false;
    rsf_streamline_host* host = nullptr;
    rsf_streamline_graphics graphics{};
    ComPtr<ID3D12CommandQueue> interop_queue;
    ComPtr<ID3D12Device> direct_device;
    ComPtr<ID3D12CommandQueue> direct_queue;
    bool native12 = false;
    std::array<ComPtr<ID3D12Resource>, 8> render_buffers;
    uint32_t render_index = 0;
    void* provider = nullptr;
    rsf_shared_surface* surface = nullptr;
    rsf_shared_fence* shared_fence = nullptr;
    ComPtr<ID3D12Fence> complete;
    // Signalled on the provider queue once vendor retirement has joined; see retire().
    ComPtr<ID3D12Fence> retired;
    uint64_t retired_serial = 0;
    struct Slot {
        ComPtr<ID3D12CommandAllocator> allocator;
        ComPtr<ID3D12GraphicsCommandList> list;
        uint64_t fence = 0;
    } slots[3];
    DXGI_SWAP_CHAIN_DESC description{};
    HANDLE completion_event = nullptr;
    uint64_t serial = 0;
    uint32_t next_slot = 0;
    bool faulted = false;
    uint64_t timing_epoch = 0, previous_present = 0, timing_frames = 0, timing_sdk_start = 0;
    double tick_ms = 0, interval_min = 0, interval_max = 0;
    std::array<double, 5> timing_sum{}, timing_max{};
    uint32_t timing_slots = 0;
    void trace_pacing(const std::array<uint64_t, 6>& ticks, uint32_t index, UINT sync, UINT physical_flags) {
        rsf_fg_status state{}; state.struct_size = sizeof(state);
        if (!settings.debug_timing || !settings.log || !tick_ms || !provider || !generation_api() ||
            generation_api()->status(provider, &state) != RSF_BACKEND_OK) return;
        const auto end = ticks.back();
        if (!timing_epoch) { timing_epoch = end; timing_sdk_start = state.total_presented; previous_present = ticks[0]; return; }
        const double interval = double(ticks[0] - previous_present) * tick_ms; previous_present = ticks[0];
        interval_min = timing_frames ? std::min(interval_min, interval) : interval;
        interval_max = std::max(interval_max, interval); ++timing_frames; timing_slots |= 1u << index;
        for (size_t i = 0; i < timing_sum.size(); ++i) {
            const auto elapsed = double(ticks[i + 1] - ticks[i]) * tick_ms;
            timing_sum[i] += elapsed; timing_max[i] = std::max(timing_max[i], elapsed);
        }
        const auto elapsed = double(end - timing_epoch) * tick_ms;
        if (elapsed < 1000) return;
        char text[640];
        std::snprintf(text, sizeof(text),
            "FG pacing frames=%llu rendered=%.1f presented=%.1f interval_ms=%.2f..%.2f callbacks_ms=%.2f/%.2f slot_wait_ms=%.2f/%.2f upload_ms=%.2f/%.2f present_ms=%.2f/%.2f sdk_ms=%.2f/%.2f slots=0x%x sync=%u active=%u physical_flags=0x%x",
            static_cast<unsigned long long>(timing_frames), double(timing_frames) * 1000 / elapsed,
            double(state.total_presented - timing_sdk_start) * 1000 / elapsed, interval_min, interval_max,
            timing_sum[0] / double(timing_frames), timing_max[0], timing_sum[1] / double(timing_frames), timing_max[1],
            timing_sum[2] / double(timing_frames), timing_max[2], timing_sum[3] / double(timing_frames), timing_max[3],
            timing_sum[4] / double(timing_frames), timing_max[4], timing_slots, sync, state.active, physical_flags);
        log(text);
        timing_epoch = end; timing_sdk_start = state.total_presented; reset_timing();
    }
    // Clears one reporting window. A zero epoch also restarts the clock on the next frame.
    void reset_timing() {
        timing_frames = 0; timing_sum = {}; timing_max = {}; interval_min = interval_max = 0; timing_slots = 0;
    }
    HRESULT wait(ID3D12Fence* fence, uint64_t value) { return rsf::wait_fence(fence, value, completion_event); }
    void report_device_failure(HRESULT result) {
        char text[160]; std::snprintf(text, sizeof(text), "Unity presentation failed backend=%u HRESULT=%08lx device_reason=%08lx",
            active_backend.load(), static_cast<unsigned long>(result), static_cast<unsigned long>(direct_device->GetDeviceRemovedReason())); log(text);
        ComPtr<ID3D12InfoQueue> messages;
        if (FAILED(direct_device.As(&messages))) return;
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
    HRESULT drain() {
        auto* queue = static_cast<ID3D12CommandQueue*>(graphics.queue);
        if (!queue || !complete) return S_OK;
        if (interop_queue && (FAILED(interop_queue->Signal(complete.Get(), ++serial)) ||
            FAILED(wait(complete.Get(), serial)))) return E_FAIL;
        if (FAILED(queue->Signal(complete.Get(), ++serial)) || FAILED(wait(complete.Get(), serial))) return E_FAIL;
        if (provider) {
            rsf_fg_retirement retirement{}; retirement.struct_size = sizeof(retirement);
            if (generation_api()->retirement(provider, &retirement) != RSF_BACKEND_OK ||
                FAILED(wait(static_cast<ID3D12Fence*>(retirement.fence), retirement.value))) return E_FAIL;
        }
        return S_OK;
    }
    HRESULT make_surface(UINT width, UINT height, DXGI_FORMAT format) {
        rsf_shared_surface_setup setup{sizeof(setup), RSF_SHARED_SURFACE_ABI_VERSION,
            width, height, uint32_t(format), 1, settings.log, settings.user};
        return rsf_shared_surface_create(device.Get(), graphics.native_device, &setup, &surface) == RSF_SHARED_OK ? S_OK : E_FAIL;
    }
    HRESULT make_render_buffers() {
        if (!description.BufferCount || description.BufferCount > render_buffers.size()) return E_INVALIDARG;
        D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
        desc.Width = description.BufferDesc.Width; desc.Height = description.BufferDesc.Height;
        desc.Format = description.BufferDesc.Format; desc.DepthOrArraySize = desc.MipLevels = 1;
        desc.SampleDesc.Count = 1; desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
        for (UINT i = 0; i < description.BufferCount; ++i)
            if (FAILED(direct_device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
                D3D12_RESOURCE_STATE_PRESENT, nullptr, IID_PPV_ARGS(&render_buffers[i])))) return E_FAIL;
        render_index = 0; return S_OK;
    }
    rsf_generation_setup generation_setup(uint32_t backend) {
        rsf_generation_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_FG_ABI_VERSION;
        setup.feature_major = backend; setup.streamline_host = host;
        setup.runtime_directory_utf8 = sdk_directory(backend).c_str();
        setup.depth_inverted = settings.depth_inverted; setup.depth_infinite = settings.depth_infinite;
        setup.view_space_to_meters = settings.units_to_meters;
        auto& chain = setup.chain; chain.struct_size = sizeof(chain); chain.abi_version = RSF_BACKEND_ABI_VERSION;
        chain.d3d12_device = graphics.device; chain.d3d12_queue = graphics.queue;
        if (backend != RSF_FG_BACKEND_DLSS) {
            chain.d3d12_device = graphics.native_device;
            chain.d3d12_queue = native12 ? direct_queue.Get() : graphics.native_queue;
        }
        chain.hwnd = description.OutputWindow; chain.width = description.BufferDesc.Width; chain.height = description.BufferDesc.Height;
        chain.format = description.BufferDesc.Format; chain.buffer_count = native12 ? description.BufferCount : 3u;
        chain.max_generated_frames = settings.max_generated_frames ? settings.max_generated_frames : 3u;
        chain.ui_mode = settings.ui_mode ? settings.ui_mode : RSF_UI_MODE_NONE; chain.allow_tearing = tearing_supported;
        chain.log = settings.log; chain.log_user = settings.user; return setup;
    }
    rsf_backend_result create_physical(uint32_t backend) {
        ComPtr<IDXGISwapChain4> replacement;
        if (!backend) {
            auto desc = description; desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD; desc.BufferCount = native12 ? description.BufferCount : 3u;
            desc.Flags = tearing_supported ? DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING : 0;
            ComPtr<IDXGISwapChain> plain;
            auto* owner_queue = native12 ? direct_queue.Get() : static_cast<ID3D12CommandQueue*>(graphics.native_queue);
            if (FAILED(original_create(parent.Get(), owner_queue, &desc, &plain)) || FAILED(plain.As(&replacement)))
                return RSF_BACKEND_ERROR_INIT_FAILED;
            publish_physical(replacement, owner_queue);
            return RSF_BACKEND_OK;
        }
        const auto* api = backend_api(backend); auto setup = generation_setup(backend);
        if (!api) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
        void* chain = nullptr;
        auto result = api->create(&setup, &provider, &chain);
        if (result == RSF_BACKEND_OK && (!chain || FAILED(static_cast<IUnknown*>(chain)->QueryInterface(IID_PPV_ARGS(&replacement)))))
            result = RSF_BACKEND_ERROR_INIT_FAILED;
        if (result == RSF_BACKEND_OK) result = api->configure(provider, &fg_off);
        if (result != RSF_BACKEND_OK) { replacement.Reset(); if (provider) api->destroy(provider); provider = nullptr; }
        else publish_physical(replacement, static_cast<ID3D12CommandQueue*>(setup.chain.d3d12_queue));
        return result;
    }
    // The provider session is the latency owner only when it does not pace through the host.
    bool paced_by_host() {
        rsf_fg_status state{}; state.struct_size = sizeof(state);
        return host && provider && generation_api() && generation_api()->status(provider, &state) == RSF_BACKEND_OK &&
            state.pacing_owner == RSF_PACING_REFLEX;
    }
    // Streamline's proxy queue takes the proxy list; any other provider queue takes the native
    // list behind it. Null when the host cannot unwrap the list.
    void* provider_list(ID3D12GraphicsCommandList* list) {
        return host && provider_queue.load() != graphics.queue ? rsf_streamline_host_native(host, list) : list;
    }
    void switch_provider(uint32_t backend) {
        if (!settings.runtime_switching || (backend == RSF_FG_BACKEND_DLSS && !host)) { switch_result.store(RSF_BACKEND_ERROR_NOT_SUPPORTED); return; }
        if (backend == active_backend.load()) { switch_result.store(0); return; }
        std::lock_guard<std::shared_mutex> lock(provider_guard);
        const auto previous = active_backend.load();
        auto result = FAILED(drain()) ? RSF_BACKEND_ERROR_NOT_READY : RSF_BACKEND_OK;
        if (result == RSF_BACKEND_OK && backend && backend != RSF_FG_BACKEND_DLSS) {
            auto setup = generation_setup(backend); setup.chain.hwnd = nullptr;
            void* probe = nullptr; void* probe_chain = nullptr;
            result = backend_api(backend)->create(&setup, &probe, &probe_chain);
            if (probe) backend_api(backend)->destroy(probe);
        }
        if (result != RSF_BACKEND_OK) { switch_result.store(result); return; }
        BOOL fullscreen = FALSE; physical->GetFullscreenState(&fullscreen, nullptr);
        if (fullscreen) physical->SetFullscreenState(FALSE, nullptr);
        clear_physical();
        active_session.store(nullptr); host_paced.store(false);
        if (provider) backend_api(previous)->destroy(provider);
        provider = nullptr;
        creating = true;
        if (host) {
            (void)rsf_streamline_host_reflex(host, RSF_REFLEX_OFF, 0);
            result = rsf_streamline_host_generation_load(host, backend == RSF_FG_BACKEND_DLSS);
        }
        if (result == RSF_BACKEND_OK) result = create_physical(backend);
        uint32_t effective = backend;
        if (result != RSF_BACKEND_OK) {
            effective = previous;
            if (host) (void)rsf_streamline_host_generation_load(host, previous == RSF_FG_BACKEND_DLSS);
            if (create_physical(previous) != RSF_BACKEND_OK) {
                effective = 0;
                if (create_physical(0) != RSF_BACKEND_OK) faulted = true;
            }
        }
        creating = false;
        if (fullscreen && physical) physical->SetFullscreenState(TRUE, nullptr);
        active_backend.store(effective); active_session.store(provider); host_paced.store(paced_by_host()); ++chain_generation;
        timing_epoch = 0; reset_timing();
        switch_result.store(result);
        char text[160]; std::snprintf(text, sizeof(text), "Generation runtime switch requested=%u effective=%u result=%d generation=%llu",
            backend, effective, result, static_cast<unsigned long long>(chain_generation.load())); log(text);
    }
    ~Bridge() {
        if (active_chain.load() == this) { active_chain.store(nullptr); active_interop.store(nullptr); active_session.store(nullptr); host_paced.store(false); }
        // On device failure retain GPU-reachable storage for the process rather than free it.
        if (FAILED(drain())) {
            log("presentation bridge: retirement failed; retaining provider and shared inputs");
            for (auto& slot : slots) { slot.list.Detach(); slot.allocator.Detach(); }
            complete.Detach(); retired.Detach(); physical.Detach(); context.Detach(); device.Detach(); parent.Detach();
            interop_queue.Detach();
            direct_device.Detach(); direct_queue.Detach();
            for (auto& buffer : render_buffers) buffer.Detach();
            return;
        }
        rsf_shared_surface_destroy(surface); rsf_shared_fence_destroy(shared_fence);
        physical.Reset();
        if (provider) generation_api()->destroy(provider);
        if (completion_event) CloseHandle(completion_event);
        if (host && active_host.load() != host) {
            interop_queue.Reset();
            for (auto& slot : slots) { slot.list.Reset(); slot.allocator.Reset(); }
            complete.Reset(); retired.Reset(); context.Reset(); device.Reset(); parent.Reset();
            rsf_streamline_host_destroy(host);
        } else if (host) interop_queue.Detach(); // Shared SR callbacks retain this process-owned queue.
        // The host also serves SR/CPU callbacks and stays loaded until process exit.
    }
    // Streamline needs the engine identity; without one there is no host and no DLSS-G.
    static bool has_identity() { return !engine_version.empty() && !project_id.empty(); }
    rsf_streamline_host_setup host_setup(void* adapter) const {
        return {sizeof(rsf_streamline_host_setup), RSF_STREAMLINE_HOST_ABI_VERSION, streamline_directory.c_str(), adapter,
            settings.engine_type, engine_version.c_str(), project_id.c_str(), 1, settings.development_runtime,
            settings.log, settings.user, RSF_SL_PROFILE_DLSS_FG, 1};
    }
public:
    void* presentation_queue() { return provider_queue.load(); }
    void* command_list(void* list) { return provider_list(static_cast<ID3D12GraphicsCommandList*>(list)); }
    // Joins vendor retirement on the provider queue, optionally runs a closed list there, and
    // signals one private fence that covers both. Present thread, between Present and the next one.
    rsf_backend_result retire(void* session, void* list, void** fence, uint64_t* value) {
        auto* queue = provider_queue.load();
        const auto* api = generation_api();
        if (!queue || !api || !session || session != provider) return RSF_BACKEND_ERROR_NOT_READY;
        // The fence exists before anything is queued, so a failure never leaves the list unsignalled.
        if (!retired && FAILED(static_cast<ID3D12Device*>(graphics.native_device)->CreateFence(0, D3D12_FENCE_FLAG_NONE,
                IID_PPV_ARGS(&retired)))) return RSF_BACKEND_ERROR_INIT_FAILED;
        rsf_fg_retirement retirement{}; retirement.struct_size = sizeof(retirement);
        const auto result = api->retirement(session, &retirement);
        if (result != RSF_BACKEND_OK) return result;
        if (retirement.fence && FAILED(queue->Wait(static_cast<ID3D12Fence*>(retirement.fence), retirement.value)))
            return RSF_BACKEND_ERROR_FEATURE_FAILED;
        if (list) {
            auto* command = static_cast<ID3D12GraphicsCommandList*>(provider_list(static_cast<ID3D12GraphicsCommandList*>(list)));
            if (!command) return RSF_BACKEND_ERROR_FEATURE_FAILED;
            ID3D12CommandList* lists[]{command}; queue->ExecuteCommandLists(1, lists);
        }
        if (FAILED(queue->Signal(retired.Get(), retired_serial + 1))) return RSF_BACKEND_ERROR_FEATURE_FAILED;
        *fence = retired.Get(); *value = ++retired_serial;
        return RSF_BACKEND_OK;
    }
    HRESULT initialize(IDXGIFactory* factory, IUnknown* source, const DXGI_SWAP_CHAIN_DESC& desc) {
        description = desc; parent = factory;
        LARGE_INTEGER frequency{}; QueryPerformanceFrequency(&frequency); tick_ms = 1000.0 / double(frequency.QuadPart);
        ComPtr<IDXGIDevice> dxgi; ComPtr<IDXGIAdapter> adapter;
        native12 = SUCCEEDED(source->QueryInterface(IID_PPV_ARGS(&direct_queue)));
        if (settings.backend == RSF_FG_BACKEND_AUTO) {
            ComPtr<IDXGIFactory4> owning_factory;
            ComPtr<IDXGIAdapter1> owning_adapter;
            if (native12) {
                if (FAILED(direct_queue->GetDevice(IID_PPV_ARGS(&direct_device))) ||
                    FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&owning_factory))) ||
                    FAILED(owning_factory->EnumAdapterByLuid(direct_device->GetAdapterLuid(), IID_PPV_ARGS(&owning_adapter)))) return E_FAIL;
            } else {
                if (FAILED(source->QueryInterface(IID_PPV_ARGS(&dxgi))) || FAILED(dxgi->GetAdapter(&adapter)) ||
                    FAILED(adapter.As(&owning_adapter))) return E_FAIL;
            }
            DXGI_ADAPTER_DESC1 info{};
            if (FAILED(owning_adapter->GetDesc1(&info))) return E_FAIL;
            const auto backend = rsf_fg_default_backend(info.VendorId, (info.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0);
            active_backend.store(backend);
            char text[160]; std::snprintf(text, sizeof(text), "Auto FG selected backend %u from owning adapter vendor=0x%x device=0x%x", backend, info.VendorId, info.DeviceId);
            log(text);
        }
        graphics.struct_size = sizeof(graphics);
        if (native12) {
            // Unity and other native D3D12 plugins use the engine's device and queue.
            if (settings.backend == RSF_FG_BACKEND_DLSS && !settings.runtime_switching) return DXGI_ERROR_UNSUPPORTED;
            if (FAILED(direct_queue->GetDevice(IID_PPV_ARGS(&direct_device)))) return E_NOINTERFACE;
            if (settings.runtime_switching && settings.streamline_directory_utf8 && !streamline_directory.empty()) {
                ComPtr<IDXGIFactory4> native_factory; ComPtr<IDXGIAdapter1> native_adapter;
                if (SUCCEEDED(CreateDXGIFactory1(IID_PPV_ARGS(&native_factory))) &&
                    SUCCEEDED(native_factory->EnumAdapterByLuid(direct_device->GetAdapterLuid(), IID_PPV_ARGS(&native_adapter)))) {
                    const auto setup = host_setup(native_adapter.Get());
                    const auto result = has_identity() ? rsf_streamline_host_adopt(&setup, direct_device.Get(), direct_queue.Get(), &host) :
                        RSF_BACKEND_ERROR_NOT_SUPPORTED;
                    // Only INIT_FAILED can follow slInit, slUpgradeInterface or slSetD3DDevice on the engine
                    // device, which may then be half registered. Every other refusal (no identity, another
                    // vendor, no SDK, missing or unsigned folder, foreign interposer) happens before the
                    // device is touched and only leaves DLSS-G unavailable.
                    if (result == RSF_BACKEND_ERROR_INIT_FAILED) { log("Unity shared Streamline registration refused"); return DXGI_ERROR_UNSUPPORTED; }
                    if (result != RSF_BACKEND_OK) {
                        char text[160]; std::snprintf(text, sizeof(text), "Unity shared Streamline host unavailable (%d); DLSS-G disabled", result);
                        log(text);
                    }
                }
            }
        } else {
            if (FAILED(source->QueryInterface(IID_PPV_ARGS(&device)))) return E_NOINTERFACE;
            ComPtr<ID3D11DeviceContext> immediate; device->GetImmediateContext(&immediate);
            if (FAILED(immediate.As(&context)) || FAILED(device.As(&dxgi)) || FAILED(dxgi->GetAdapter(&adapter))) return E_NOINTERFACE;
        }
        // The host decides which adapters Streamline serves. Only a strict DLSS request (no runtime
        // switching) needs it; otherwise a refusal leaves FidelityFX, XeSS and Off available.
        const bool dlss_required = !settings.runtime_switching && (!settings.backend || settings.backend == RSF_FG_BACKEND_DLSS);
        if (!native12 && (dlss_required || settings.runtime_switching)) {
            const auto setup = host_setup(adapter.Get());
            const auto host_result = has_identity() ? rsf_streamline_host_create(&setup, &host) : RSF_BACKEND_ERROR_NOT_SUPPORTED;
            if (host_result != RSF_BACKEND_OK) {
                char text[160]; std::snprintf(text, sizeof(text), "Streamline FG host refused (%d); %s", host_result,
                    dlss_required ? "retaining D3D11 presentation" : "DLSS-G unavailable, other generation backends remain");
                log(text);
                if (dlss_required) return DXGI_ERROR_UNSUPPORTED;
            }
        }
        if (host) {
            if (rsf_streamline_host_graphics(host, &graphics) != RSF_BACKEND_OK) return E_FAIL;
        } else {
            if (!native12 && FAILED(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0,
                    IID_PPV_ARGS(&direct_device)))) return DXGI_ERROR_UNSUPPORTED;
            if (!direct_queue) {
                D3D12_COMMAND_QUEUE_DESC queue{}; queue.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
                if (FAILED(direct_device->CreateCommandQueue(&queue, IID_PPV_ARGS(&direct_queue)))) return E_FAIL;
            }
            graphics.device = graphics.native_device = direct_device.Get();
            graphics.queue = graphics.native_queue = direct_queue.Get();
        }
        ComPtr<IDXGIFactory5> capability_factory;
        BOOL allow_tearing = FALSE;
        tearing_supported = SUCCEEDED(factory->QueryInterface(IID_PPV_ARGS(&capability_factory))) &&
            SUCCEEDED(capability_factory->CheckFeatureSupport(DXGI_FEATURE_PRESENT_ALLOW_TEARING,
                &allow_tearing, sizeof(allow_tearing))) && allow_tearing;
        auto* gpu = static_cast<ID3D12Device*>(graphics.device);
        D3D12_COMMAND_QUEUE_DESC interop_desc{}; interop_desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
        if (native12) interop_queue = direct_queue;
        else if (FAILED(gpu->CreateCommandQueue(&interop_desc, IID_PPV_ARGS(&interop_queue)))) return E_FAIL;
        completion_event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        if (!completion_event || FAILED(gpu->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&complete)))) return E_FAIL;
        if (!native12 && (rsf_shared_fence_create(device.Get(), graphics.native_device, settings.log, settings.user, &shared_fence) != RSF_SHARED_OK ||
            FAILED(make_surface(desc.BufferDesc.Width, desc.BufferDesc.Height, desc.BufferDesc.Format)))) return E_FAIL;
        for (auto& slot : slots)
            if (!rsf::create_command_list(gpu, D3D12_COMMAND_LIST_TYPE_DIRECT, slot.allocator.ReleaseAndGetAddressOf(),
                    slot.list.ReleaseAndGetAddressOf())) return E_FAIL;
        if (settings.runtime_switching) {
            if (native12 && FAILED(make_render_buffers())) return E_FAIL;
            if (host && active_backend.load() != RSF_FG_BACKEND_DLSS && rsf_streamline_host_generation_load(host, 0) != RSF_BACKEND_OK) return E_FAIL;
            const auto result = create_physical(active_backend.load());
            if (result != RSF_BACKEND_OK) {
                switch_result.store(result); active_backend.store(0);
                if (create_physical(0) != RSF_BACKEND_OK) return E_FAIL;
            }
        } else {
            const auto result = create_physical(active_backend.load());
            if (result != RSF_BACKEND_OK) {
                char text[128]; std::snprintf(text, sizeof(text), "Generation backend=%u create refused (%d); retaining engine presentation", active_backend.load(), result);
                log(text); return E_FAIL;
            }
        }
        if (!desc.Windowed && FAILED(physical->SetFullscreenState(TRUE, nullptr))) return E_FAIL;
        log(tearing_supported ? "presentation bridge: windowed flip chain supports immediate tearing presentation" :
            "presentation bridge: DXGI tearing unavailable; using supported flip presentation");
        active_host.store(host, std::memory_order_release);
        active_graphics = graphics;
        active_session.store(provider, std::memory_order_release);
        host_paced.store(paced_by_host());
        active_interop.store(interop_queue.Get(), std::memory_order_release);
        active_chain.store(this, std::memory_order_release);
        rsf_fg_status status{}; status.struct_size = sizeof(status);
        if (settings.log && provider && generation_api()->status(provider, &status) == RSF_BACKEND_OK) {
            char text[240]; std::snprintf(text, sizeof(text), "Generation support=%u max generated=%u dynamic=%u VSync=%u minimum=%u latency available=%u version=%s",
                status.supported, status.max_generated_frames, status.dynamic_supported, status.vsync_supported,
                status.min_dimension, status.low_latency_available, status.version_name); log(text);
        }
        log(native12 ? "presentation bridge: native D3D12 engine queue, one vendor generation chain" :
            "presentation bridge: D3D11 facade, one vendor D3D12 generation chain, separate interop queue");
        return S_OK;
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
    ULONG STDMETHODCALLTYPE Release() override { const auto value = --references; if (!value) delete this; return value; }
    HRESULT STDMETHODCALLTYPE SetPrivateData(REFGUID key, UINT bytes, const void* data) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->SetPrivateData(key, bytes, data); }); }
    HRESULT STDMETHODCALLTYPE SetPrivateDataInterface(REFGUID key, const IUnknown* data) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->SetPrivateDataInterface(key, data); }); }
    HRESULT STDMETHODCALLTYPE GetPrivateData(REFGUID key, UINT* bytes, void* data) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->GetPrivateData(key, bytes, data); }); }
    HRESULT STDMETHODCALLTYPE GetParent(REFIID iid, void** out) override { return parent->QueryInterface(iid, out); }
    HRESULT STDMETHODCALLTYPE GetDevice(REFIID iid, void** out) override { return native12 ? direct_device->QueryInterface(iid, out) : device->QueryInterface(iid, out); }
    // Physical Present and the provider callbacks around it, after the frame's list was submitted.
    HRESULT present_physical(rsf_observer_present_event& event, UINT sync, UINT physical_flags, std::array<uint64_t, 6>& ticks) {
        if (settings.latency_event) settings.latency_event(settings.user, &event);
        if (settings.debug_timing) ticks[3] = clock_tick();
        const auto result = physical->Present(sync, physical_flags);
        if (settings.debug_timing) ticks[4] = clock_tick();
        event.completed = 1; event.result = result;
        if (settings.latency_event) settings.latency_event(settings.user, &event);
        if (provider) generation_api()->after_present(provider);
        if (settings.retire) settings.retire(settings.user, provider);
        return result;
    }
    void notify_presented(rsf_observer_present_event& event, HRESULT result) {
        event.completed = 1; event.result = result;
        rsf_observer_notify_application_present(&event);
        if (settings.present_event) settings.present_event(settings.user, &event);
    }
    // A requested provider change happens here, at a drained Present boundary.
    void switch_if_requested() {
        const auto selected = requested_backend.exchange(UINT32_MAX);
        if (selected != UINT32_MAX) switch_provider(selected);
    }
    HRESULT STDMETHODCALLTYPE Present(UINT sync, UINT flags) override {
        if (faulted) return DXGI_ERROR_DEVICE_REMOVED;
        if (flags & DXGI_PRESENT_TEST) return query_physical([&](IDXGISwapChain4* chain) { return chain->Present(sync, flags); });
        // Legacy discard chains cannot request the flip-model tearing creation flag.
        // Translate VSync Off to the supported DX12/windowed immediate-present contract.
        BOOL fullscreen = FALSE;
        UINT physical_flags = flags & ~DXGI_PRESENT_ALLOW_TEARING;
        if (!sync && tearing_supported && SUCCEEDED(physical->GetFullscreenState(&fullscreen, nullptr)) && !fullscreen)
            physical_flags |= DXGI_PRESENT_ALLOW_TEARING;
        std::array<uint64_t, 6> ticks{};
        if (settings.debug_timing) ticks[0] = clock_tick();
        rsf_observer_present_event event{sizeof(event), 0, this, sync, flags, 0, 0};
        rsf_observer_notify_application_present(&event);
        if (settings.present_event) settings.present_event(settings.user, &event);
        if (settings.before_present) settings.before_present(settings.user, this);
        if (settings.debug_timing) ticks[1] = clock_tick();
        HRESULT result = E_FAIL;
        // Fence serial advances three times per frame. It cannot also select the three-slot
        // command ring: serial % 3 would reuse one slot and wait for the GPU every frame.
        const auto index = next_slot; next_slot = (next_slot + 1) % 3;
        auto& slot = slots[index];
        if (native12) {
            auto* present_queue = provider_queue.load();
            // Streamline requires CreateCommandQueue through its upgraded device, so its queue is
            // not the engine queue. Join the engine's source work into it, and the copy back out.
            const bool joined = present_queue != direct_queue.Get();
            if (joined) {
                const auto source_ready = ++serial;
                if (FAILED(direct_queue->Signal(complete.Get(), source_ready)) ||
                    FAILED(present_queue->Wait(complete.Get(), source_ready))) return DXGI_ERROR_DEVICE_REMOVED;
            }
            if (SUCCEEDED(wait(complete.Get(), slot.fence)) && SUCCEEDED(slot.allocator->Reset()) &&
                SUCCEEDED(slot.list->Reset(slot.allocator.Get(), nullptr))) {
                ComPtr<ID3D12Resource> buffer;
                if (SUCCEEDED(physical->GetBuffer(physical->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&buffer)))) {
                    if (settings.runtime_switching)
                        rsf::copy_transitioned(slot.list.Get(), buffer.Get(), D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_PRESENT,
                            render_buffers[render_index].Get(), D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_PRESENT);
                    void* command = provider_list(slot.list.Get());
                    if (settings.prepare) settings.prepare(settings.user, this, nullptr, command, buffer.Get(), host, provider, sync);
                    if (SUCCEEDED(slot.list->Close())) {
                        ID3D12CommandList* lists[]{static_cast<ID3D12GraphicsCommandList*>(command)}; present_queue->ExecuteCommandLists(1, lists);
                        if (joined) {
                            const auto copied = ++serial;
                            if (FAILED(present_queue->Signal(complete.Get(), copied)) ||
                                FAILED(direct_queue->Wait(complete.Get(), copied))) return DXGI_ERROR_DEVICE_REMOVED;
                        }
                        result = present_physical(event, sync, physical_flags, ticks);
                    }
                }
                slot.fence = ++serial;
                if (FAILED(present_queue->Signal(complete.Get(), slot.fence))) result = DXGI_ERROR_DEVICE_REMOVED;
            }
            notify_presented(event, result);
            if (SUCCEEDED(result) && settings.runtime_switching) {
                ++application_presents;
                render_index = (render_index + 1) % description.BufferCount;
                switch_if_requested();
            }
            if (FAILED(result)) { report_device_failure(result); faulted = true; }
            return result;
        }
        if (SUCCEEDED(wait(complete.Get(), slot.fence)) && SUCCEEDED(slot.allocator->Reset()) &&
            SUCCEEDED(slot.list->Reset(slot.allocator.Get(), nullptr))) {
            if (settings.debug_timing) ticks[2] = clock_tick();
            // Real DXGI backbuffers (the Off path) must be written on their creation queue.
            // Keep the separate interop queue for SR; final upload uses the present queue.
            auto* queue = static_cast<ID3D12CommandQueue*>(graphics.queue);
            auto* fence12 = static_cast<ID3D12Fence*>(rsf_shared_fence_d3d12(shared_fence));
            auto* fence11 = static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(shared_fence));
            // One facade render target is written by AC7. The shared copy is read only for the
            // queued upload; generated-frame consumers use the physical backbuffer instead.
            const uint64_t upload = ++serial;
            const auto signaled = context->Signal(fence11, upload);
            // Submit the signal with its preceding D3D11 work before D3D12 waits on it.
            // Flushing before Signal leaves the dependency buffered until a later frame.
            if (SUCCEEDED(signaled)) context->Flush();
            if (SUCCEEDED(signaled) && SUCCEEDED(queue->Wait(fence12, upload))) {
                ComPtr<ID3D12Resource> buffer;
                if (SUCCEEDED(physical->GetBuffer(physical->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&buffer)))) {
                    rsf::copy_transitioned(slot.list.Get(), buffer.Get(), D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_PRESENT,
                        static_cast<ID3D12Resource*>(rsf_shared_surface_d3d12(surface)), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COMMON);
                    void* command = provider_list(slot.list.Get());
                    if (settings.prepare) settings.prepare(settings.user, this, context.Get(), command, buffer.Get(), host, provider, sync);
                    if (SUCCEEDED(slot.list->Close())) {
                        ID3D12CommandList* lists[]{slot.list.Get()}; queue->ExecuteCommandLists(1, lists);
                        const auto copied = ++serial;
                        if (SUCCEEDED(queue->Signal(fence12, copied)) && SUCCEEDED(queue->Wait(fence12, copied)) &&
                            SUCCEEDED(context->Wait(fence11, copied)))
                            result = present_physical(event, sync, physical_flags, ticks);
                    }
                }
            }
            slot.fence = ++serial;
            if (FAILED(queue->Signal(complete.Get(), slot.fence))) result = DXGI_ERROR_DEVICE_REMOVED;
        }
        notify_presented(event, result);
        if (settings.debug_timing && SUCCEEDED(result)) { ticks[5] = clock_tick(); trace_pacing(ticks, index, sync, physical_flags); }
        if (SUCCEEDED(result) && settings.runtime_switching) switch_if_requested();
        if (FAILED(result)) faulted = true;
        return result;
    }
    HRESULT STDMETHODCALLTYPE GetBuffer(UINT index, REFIID iid, void** out) override {
        if (native12 && settings.runtime_switching) {
            if (!out) return E_POINTER; *out = nullptr;
            return index < description.BufferCount && render_buffers[index] ? render_buffers[index]->QueryInterface(iid, out) : DXGI_ERROR_INVALID_CALL;
        }
        if (native12) return query_physical([&](IDXGISwapChain4* chain) { return chain->GetBuffer(index, iid, out); });
        if (!out) return E_POINTER;
        *out = nullptr;
        if (index || !surface) return DXGI_ERROR_INVALID_CALL;
        return static_cast<IUnknown*>(rsf_shared_surface_d3d11(surface))->QueryInterface(iid, out);
    }
    HRESULT STDMETHODCALLTYPE SetFullscreenState(BOOL enabled, IDXGIOutput* target) override {
        if (FAILED(drain())) return E_FAIL;
        const auto chain = current_physical(); return chain ? chain->SetFullscreenState(enabled, target) : DXGI_ERROR_WAS_STILL_DRAWING;
    }
    HRESULT STDMETHODCALLTYPE GetFullscreenState(BOOL* enabled, IDXGIOutput** target) override {
        return query_physical([&](IDXGISwapChain4* chain) { return chain->GetFullscreenState(enabled, target); });
    }
    HRESULT STDMETHODCALLTYPE GetDesc(DXGI_SWAP_CHAIN_DESC* out) override { if (!out) return E_POINTER; if (native12 && !settings.runtime_switching) return query_physical([&](IDXGISwapChain4* chain) { return chain->GetDesc(out); }); *out = description; return S_OK; }
    HRESULT STDMETHODCALLTYPE ResizeBuffers(UINT count, UINT width, UINT height, DXGI_FORMAT format, UINT flags) override {
        if ((!native12 && count > 1) || faulted) return DXGI_ERROR_INVALID_CALL;
        if (!width || !height) { RECT rect{}; if (!GetClientRect(description.OutputWindow, &rect)) return E_FAIL; width = UINT(rect.right); height = UINT(rect.bottom); }
        if (!width || !height) return DXGI_ERROR_INVALID_CALL;
        if (format == DXGI_FORMAT_UNKNOWN) format = description.BufferDesc.Format;
        if (FAILED(drain())) return E_FAIL;
        if (!native12) {
            auto* texture = static_cast<IUnknown*>(rsf_shared_surface_d3d11(surface));
            texture->AddRef(); if (texture->Release() > 1) return DXGI_ERROR_INVALID_CALL;
        }
        if (provider && generation_api()->configure(provider, &fg_off) != RSF_BACKEND_OK) return E_FAIL;
        if (native12 && settings.runtime_switching) {
            for (const auto& buffer : render_buffers) if (buffer) {
                buffer->AddRef(); if (buffer->Release() > 1) return DXGI_ERROR_INVALID_CALL;
            }
            if (count > render_buffers.size()) return DXGI_ERROR_INVALID_CALL;
        }
        rsf_shared_surface_destroy(surface); surface = nullptr;
        const auto result = physical->ResizeBuffers(native12 ? count : 3, width, height, format,
            tearing_supported ? DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING : 0);
        if (FAILED(result)) { if (!native12) make_surface(description.BufferDesc.Width, description.BufferDesc.Height, description.BufferDesc.Format); return result; }
        if (!native12 && FAILED(make_surface(width, height, format))) { faulted = true; return E_FAIL; }
        description.BufferDesc.Width = width; description.BufferDesc.Height = height; description.BufferDesc.Format = format; description.Flags = flags;
        if (native12 && settings.runtime_switching) {
            if (count) description.BufferCount = count;
            for (auto& buffer : render_buffers) buffer.Reset();
            if (FAILED(make_render_buffers())) { faulted = true; return E_FAIL; }
        }
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE ResizeTarget(const DXGI_MODE_DESC* desc) override {
        const auto chain = current_physical(); return chain ? chain->ResizeTarget(desc) : DXGI_ERROR_WAS_STILL_DRAWING;
    }
    HRESULT STDMETHODCALLTYPE GetContainingOutput(IDXGIOutput** out) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->GetContainingOutput(out); }); }
    HRESULT STDMETHODCALLTYPE GetFrameStatistics(DXGI_FRAME_STATISTICS* out) override {
        const auto result = query_physical([&](IDXGISwapChain4* chain) { return chain->GetFrameStatistics(out); });
        if (SUCCEEDED(result) && out && native12 && settings.runtime_switching) out->PresentCount = application_presents.load();
        return result;
    }
    HRESULT STDMETHODCALLTYPE GetLastPresentCount(UINT* out) override {
        if (native12 && settings.runtime_switching) {
            if (!out) return E_POINTER; *out = application_presents.load(); return S_OK;
        }
        return query_physical([&](IDXGISwapChain4* chain) { return chain->GetLastPresentCount(out); });
    }
    HRESULT STDMETHODCALLTYPE GetDesc1(DXGI_SWAP_CHAIN_DESC1* out) override { if (!out) return E_POINTER; const auto result = query_physical([&](IDXGISwapChain4* chain) { return chain->GetDesc1(out); }); if (SUCCEEDED(result) && (!native12 || settings.runtime_switching)) { out->BufferCount = native12 ? description.BufferCount : 1; out->SwapEffect = description.SwapEffect; out->Flags = description.Flags; } return result; }
    HRESULT STDMETHODCALLTYPE GetFullscreenDesc(DXGI_SWAP_CHAIN_FULLSCREEN_DESC* out) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->GetFullscreenDesc(out); }); }
    HRESULT STDMETHODCALLTYPE GetHwnd(HWND* out) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->GetHwnd(out); }); }
    HRESULT STDMETHODCALLTYPE GetCoreWindow(REFIID, void** out) override { if (out) *out = nullptr; return E_NOINTERFACE; }
    HRESULT STDMETHODCALLTYPE Present1(UINT sync, UINT flags, const DXGI_PRESENT_PARAMETERS* params) override { if (params && (params->DirtyRectsCount || params->pScrollRect)) return DXGI_ERROR_INVALID_CALL; return Present(sync, flags); }
    BOOL STDMETHODCALLTYPE IsTemporaryMonoSupported() override {
        std::lock_guard<std::mutex> lock(physical_guard); return physical ? physical->IsTemporaryMonoSupported() : FALSE;
    }
    HRESULT STDMETHODCALLTYPE GetRestrictToOutput(IDXGIOutput** out) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->GetRestrictToOutput(out); }); }
    HRESULT STDMETHODCALLTYPE SetBackgroundColor(const DXGI_RGBA* color) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->SetBackgroundColor(color); }); }
    HRESULT STDMETHODCALLTYPE GetBackgroundColor(DXGI_RGBA* color) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->GetBackgroundColor(color); }); }
    HRESULT STDMETHODCALLTYPE SetRotation(DXGI_MODE_ROTATION value) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->SetRotation(value); }); }
    HRESULT STDMETHODCALLTYPE GetRotation(DXGI_MODE_ROTATION* value) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->GetRotation(value); }); }
    HRESULT STDMETHODCALLTYPE SetSourceSize(UINT width, UINT height) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->SetSourceSize(width, height); }); }
    HRESULT STDMETHODCALLTYPE GetSourceSize(UINT* width, UINT* height) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->GetSourceSize(width, height); }); }
    HRESULT STDMETHODCALLTYPE SetMaximumFrameLatency(UINT value) override { return native12 ? query_physical([&](IDXGISwapChain4* chain) { return chain->SetMaximumFrameLatency(value); }) : DXGI_ERROR_INVALID_CALL; }
    HRESULT STDMETHODCALLTYPE GetMaximumFrameLatency(UINT* out) override { if (native12) return query_physical([&](IDXGISwapChain4* chain) { return chain->GetMaximumFrameLatency(out); }); if (!out) return E_POINTER; *out = 1; return S_OK; }
    HANDLE STDMETHODCALLTYPE GetFrameLatencyWaitableObject() override {
        std::lock_guard<std::mutex> lock(physical_guard); return native12 && physical ? physical->GetFrameLatencyWaitableObject() : nullptr;
    }
    HRESULT STDMETHODCALLTYPE SetMatrixTransform(const DXGI_MATRIX_3X2_F* value) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->SetMatrixTransform(value); }); }
    HRESULT STDMETHODCALLTYPE GetMatrixTransform(DXGI_MATRIX_3X2_F* value) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->GetMatrixTransform(value); }); }
    UINT STDMETHODCALLTYPE GetCurrentBackBufferIndex() override {
        if (!native12) return 0;
        if (settings.runtime_switching) return render_index;
        std::lock_guard<std::mutex> lock(physical_guard); return physical ? physical->GetCurrentBackBufferIndex() : 0;
    }
    HRESULT STDMETHODCALLTYPE CheckColorSpaceSupport(DXGI_COLOR_SPACE_TYPE value, UINT* out) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->CheckColorSpaceSupport(value, out); }); }
    HRESULT STDMETHODCALLTYPE SetColorSpace1(DXGI_COLOR_SPACE_TYPE value) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->SetColorSpace1(value); }); }
    HRESULT STDMETHODCALLTYPE ResizeBuffers1(UINT count, UINT width, UINT height, DXGI_FORMAT format, UINT flags, const UINT* masks, IUnknown* const* queues) override { if (masks || queues) return DXGI_ERROR_INVALID_CALL; return ResizeBuffers(count, width, height, format, flags); }
    HRESULT STDMETHODCALLTYPE SetHDRMetaData(DXGI_HDR_METADATA_TYPE type, UINT bytes, void* data) override { return query_physical([&](IDXGISwapChain4* chain) { return chain->SetHDRMetaData(type, bytes, data); }); }
};

HRESULT create_bridge(IDXGIFactory* factory, IUnknown* unknown, const DXGI_SWAP_CHAIN_DESC& desc, IDXGISwapChain** out) {
    if (!settings.accept_window || !settings.accept_window(settings.user, desc.OutputWindow)) return DXGI_ERROR_UNSUPPORTED;
    if (creating || active_chain.load() || !out || desc.SampleDesc.Count != 1 ||
        !desc.BufferDesc.Width || !desc.BufferDesc.Height || !IsWindow(desc.OutputWindow) ||
        (desc.BufferDesc.Format != DXGI_FORMAT_R8G8B8A8_UNORM && desc.BufferDesc.Format != DXGI_FORMAT_B8G8R8A8_UNORM &&
         desc.BufferDesc.Format != DXGI_FORMAT_R10G10B10A2_UNORM && desc.BufferDesc.Format != DXGI_FORMAT_R16G16B16A16_FLOAT)) return DXGI_ERROR_UNSUPPORTED;
    if (!unknown) return DXGI_ERROR_UNSUPPORTED;
    ComPtr<ID3D12CommandQueue> queue;
    if (FAILED(unknown->QueryInterface(IID_PPV_ARGS(&queue))) && desc.BufferCount != 1) return DXGI_ERROR_UNSUPPORTED;
    std::lock_guard<std::mutex> lock(creation_guard);
    if (active_chain.load()) return DXGI_ERROR_UNSUPPORTED;
    creating = true;
    auto* bridge = new(std::nothrow) Bridge;
    HRESULT result = bridge ? bridge->initialize(factory, unknown, desc) : E_OUTOFMEMORY;
    if (SUCCEEDED(result)) *out = bridge;
    else if (bridge) bridge->Release();
    creating = false;
    return result;
}
HRESULT STDMETHODCALLTYPE create_hook(IDXGIFactory* factory, IUnknown* device, DXGI_SWAP_CHAIN_DESC* desc, IDXGISwapChain** out) {
    if (desc && SUCCEEDED(create_bridge(factory, device, *desc, out))) return S_OK;
    return original_create(factory, device, desc, out);
}
HRESULT STDMETHODCALLTYPE hwnd_hook(IDXGIFactory2* factory, IUnknown* device, HWND hwnd,
    const DXGI_SWAP_CHAIN_DESC1* desc, const DXGI_SWAP_CHAIN_FULLSCREEN_DESC* fullscreen, IDXGIOutput* output, IDXGISwapChain1** out) {
    if (desc && !output && !desc->Stereo) {
        DXGI_SWAP_CHAIN_DESC translated{}; translated.BufferDesc = {desc->Width, desc->Height, {}, desc->Format, DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED, DXGI_MODE_SCALING_UNSPECIFIED};
        translated.SampleDesc = desc->SampleDesc; translated.BufferUsage = desc->BufferUsage; translated.BufferCount = desc->BufferCount;
        translated.OutputWindow = hwnd; translated.Windowed = fullscreen ? fullscreen->Windowed : TRUE; translated.SwapEffect = desc->SwapEffect; translated.Flags = desc->Flags;
        ComPtr<IDXGISwapChain> bridge;
        if (SUCCEEDED(create_bridge(factory, device, translated, &bridge))) return bridge->QueryInterface(IID_PPV_ARGS(out));
    }
    return original_hwnd(factory, device, hwnd, desc, fullscreen, output, out);
}
}
extern "C" int rsf_d3d11_present_install(const rsf_d3d11_present_setup* setup) {
    if (!setup || setup->struct_size < sizeof(*setup) || !setup->runtime_directory_utf8 || !setup->accept_window || original_create) return 0;
    ComPtr<IDXGIFactory2> factory;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return 0;
    // Depth units are the plugin's to state; the bridge no longer guesses them from the API.
    if (!(setup->units_to_meters > 0)) {
        if (setup->log) setup->log(setup->user, "presentation bridge: units_to_meters must be positive; interception refused");
        return 0;
    }
    if (setup->backend && !rsf_fg_backend_known(setup->backend) &&
        !(setup->backend == RSF_FG_BACKEND_AUTO && setup->runtime_switching)) return 0;
    settings = *setup; runtime_directory = setup->runtime_directory_utf8;
    engine_version = setup->engine_version_utf8 ? setup->engine_version_utf8 : "";
    project_id = setup->project_id_utf8 ? setup->project_id_utf8 : "";
    settings.engine_version_utf8 = settings.project_id_utf8 = nullptr;
    fsr3_directory = setup->fsr3_directory_utf8 ? setup->fsr3_directory_utf8 : runtime_directory;
    fsr4_directory = setup->fsr4_directory_utf8 ? setup->fsr4_directory_utf8 : runtime_directory;
    xess_directory = setup->xess_directory_utf8 ? setup->xess_directory_utf8 : runtime_directory;
    streamline_directory = setup->streamline_directory_utf8 ? setup->streamline_directory_utf8 : runtime_directory;
    active_backend.store(setup->backend ? setup->backend : setup->runtime_switching ? 0u : RSF_FG_BACKEND_DLSS);
    const auto initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return 0;
    auto** table = *reinterpret_cast<void***>(factory.Get());
    if (MH_CreateHook(table[10], reinterpret_cast<void*>(&create_hook), reinterpret_cast<void**>(&original_create)) != MH_OK) return 0;
    if (MH_CreateHook(table[15], reinterpret_cast<void*>(&hwnd_hook), reinterpret_cast<void**>(&original_hwnd)) != MH_OK) { MH_RemoveHook(table[10]); original_create = nullptr; return 0; }
    if (MH_EnableHook(table[10]) != MH_OK || MH_EnableHook(table[15]) != MH_OK) { MH_DisableHook(table[10]); MH_RemoveHook(table[10]); MH_RemoveHook(table[15]); original_create = nullptr; return 0; }
    log("presentation bridge: cold-start D3D11 swapchain interception installed"); return 1;
}
extern "C" rsf_streamline_host* rsf_d3d11_present_host() { return active_host.load(std::memory_order_acquire); }
extern "C" int rsf_d3d11_present_graphics(rsf_streamline_graphics* graphics) {
    auto* queue = active_interop.load(std::memory_order_acquire);
    if (!graphics || graphics->struct_size < sizeof(*graphics) || !queue) return 0;
    *graphics = active_graphics;
    graphics->queue = queue; graphics->native_queue = nullptr; return 1;
}
extern "C" void* rsf_d3d11_present_queue() {
    auto* bridge = static_cast<Bridge*>(active_chain.load(std::memory_order_acquire));
    if (!bridge) return nullptr;
    return bridge->presentation_queue();
}
extern "C" int rsf_d3d11_present_is_owner(void* swapchain) {
    return swapchain && active_chain.load(std::memory_order_acquire) == swapchain;
}
extern "C" int rsf_d3d11_present_has_owner() { return active_chain.load(std::memory_order_acquire) != nullptr; }
extern "C" const rsf_generation_provider* rsf_d3d11_present_provider() { return generation_api(); }
extern "C" void* rsf_d3d11_present_session() { return active_session.load(std::memory_order_acquire); }
extern "C" uint32_t rsf_d3d11_present_backend() { return active_backend.load(); }
extern "C" int32_t rsf_d3d11_present_request(uint32_t backend) {
    if (!settings.runtime_switching || !active_chain.load()) return RSF_BACKEND_ERROR_NOT_READY;
    if (backend != 0 && !rsf_fg_backend_known(backend)) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    switch_result.store(0); requested_backend.store(backend); return RSF_BACKEND_OK;
}
extern "C" int32_t rsf_d3d11_present_switch_result() { return switch_result.load(); }
extern "C" uint64_t rsf_d3d11_present_generation() { return chain_generation.load(); }
namespace {
// Main-thread latency calls share the provider lifetime; only replacement takes the exclusive lock,
// so a caller arriving mid-switch is turned away instead of blocking window messages.
template<class Call> int32_t with_provider(Call call) {
    std::shared_lock<std::shared_mutex> lock(provider_guard, std::try_to_lock);
    if (!lock.owns_lock()) return RSF_BACKEND_ERROR_NOT_READY;
    return call(generation_api(), active_session.load(), active_host.load());
}
}
extern "C" int32_t rsf_d3d11_present_retire(void* session, void* list, void** fence, uint64_t* value) {
    if (!fence || !value) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *fence = nullptr; *value = 0;
    return with_provider([&](const rsf_generation_provider*, void*, rsf_streamline_host*) -> int32_t {
        auto* bridge = static_cast<Bridge*>(active_chain.load(std::memory_order_acquire));
        return bridge ? bridge->retire(session, list, fence, value) : RSF_BACKEND_ERROR_NOT_READY;
    });
}
extern "C" void* rsf_d3d11_present_command_list(void* list) {
    auto* bridge = static_cast<Bridge*>(active_chain.load(std::memory_order_acquire));
    return bridge && list ? bridge->command_list(list) : nullptr;
}
// A provider that paces through the host is the host: it gets the host calls once. Any other
// provider owns its own pacing, and the host only records markers for latency reporting.
extern "C" int32_t rsf_d3d11_present_begin(uint64_t id, uint64_t* generation) {
    return with_provider([&](const rsf_generation_provider* api, void* context, rsf_streamline_host* host) -> int32_t {
        if (generation) *generation = chain_generation.load();
        if (host_paced.load()) return rsf_streamline_host_sleep(host, id);
        return api && context ? api->begin_frame(context, id) : RSF_BACKEND_OK;
    });
}
extern "C" int32_t rsf_d3d11_present_marker(uint64_t generation, uint64_t id, rsf_latency_marker marker, uint32_t controller) {
    return with_provider([&](const rsf_generation_provider* api, void* context, rsf_streamline_host* host) -> int32_t {
        if (generation != chain_generation.load()) return RSF_BACKEND_ERROR_NOT_READY;
        const auto sl = host ? rsf_streamline_host_marker(host, id, marker, controller) : RSF_BACKEND_OK;
        return sl != RSF_BACKEND_OK || host_paced.load() || !api || !context ? sl : api->marker(context, marker, id, controller);
    });
}
extern "C" int32_t rsf_d3d11_present_acquire(uint64_t id) {
    return with_provider([&](const rsf_generation_provider*, void*, rsf_streamline_host* host) -> int32_t {
        return host ? rsf_streamline_host_acquire(host, id) : RSF_BACKEND_OK;
    });
}
extern "C" int32_t rsf_d3d11_present_input(uint64_t id, uint32_t kinds, uint32_t message) {
    return with_provider([&](const rsf_generation_provider*, void*, rsf_streamline_host* host) -> int32_t {
        return host ? rsf_streamline_host_input(host, id, kinds, message) : RSF_BACKEND_OK;
    });
}
extern "C" int32_t rsf_d3d11_present_abort(uint64_t id) {
    return with_provider([&](const rsf_generation_provider* api, void* context, rsf_streamline_host* host) -> int32_t {
        const auto sl = host ? rsf_streamline_host_abort(host, id) : RSF_BACKEND_OK;
        return host_paced.load() || !api || !context ? sl : api->abort_frame(context, id);
    });
}
