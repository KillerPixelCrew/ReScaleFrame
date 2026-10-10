// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/unity_config.h>
#include <rescaleframe/fg_choice.h>
#include <rescaleframe/unity_sr_host.h>
#include "../../backends/common/d3d12_helpers.h"
#include <windows.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <dxgi.h>
#include <dxgi1_4.h>
#include <cstring>
#include <cwchar>
#include <wrl/client.h>
#include <array>
#include <memory>
#include <mutex>
#include <string>
#include <cstdio>
#include <algorithm>
#include <thread>
#include <vector>

namespace {
using Microsoft::WRL::ComPtr;
std::mutex guard;
rsf_native_fg_options options{sizeof(options), RSF_FG_OFF, 1, RSF_REFLEX_OFF, 0, 0};
rsf_native_fg_status state{};
std::string directory;
std::wstring log_path;
bool installed = false;
rsf_backend_log_fn logger = nullptr;
void* logger_user = nullptr;
struct Cpu {
    uint64_t id = 0, input = 0, generation = 0;
    bool begun = false, simulation = false, failed = false, render_started = false, completed = false;
};
std::array<Cpu, 128> cpu;
struct Slot {
    rsf_frame_record record{};
    ComPtr<ID3D12Resource> depth, motion, hudless;
    D3D12_RESOURCE_STATES depth_state = D3D12_RESOURCE_STATE_COMMON;
    D3D12_RESOURCE_STATES motion_state = D3D12_RESOURCE_STATE_COMMON;
    D3D12_RESOURCE_STATES hudless_state = D3D12_RESOURCE_STATE_COMMON;
    ComPtr<ID3D12Fence> completion;
    uint64_t value = 0;
    // Retirement failed, so nothing says when the vendor stopped reading. Held until a later
    // retirement covers it or the provider generation changes (a drained transition).
    bool unknown = false;
    uint64_t unknown_generation = 0;
    bool ready = false;
    bool submitted = false;
    bool hudless_ready = false;
    bool hudless_submitted = false;
};
std::array<Slot, 6> slots;
// Render callbacks and Present can run on different Unity graphics threads.
// Bind the producer identity to the engine buffer, rather than to callback TLS.
struct Window { uint64_t frame = 0, session = 0; uint32_t view = 0; };
std::array<Window, 8> windows;
thread_local uint64_t window_frame = 0;
thread_local uint64_t window_session = 0;
thread_local uint32_t window_view = 0;
thread_local uint64_t window_generation = 0;
thread_local Slot* consumed = nullptr;
uint32_t ui_mode = RSF_UI_MODE_NONE;
uint64_t diagnostics = 0;
uint64_t status_generation = 0, status_samples = 0, prepare_failures = 0;
struct ColorCapture {
    ComPtr<ID3D12Resource> buffers[2];
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprints[2]{};
    ComPtr<ID3D12Fence> completion;
    uint64_t value = 0, frame = 0, generation = 0;
    uint32_t width = 0, height = 0;
    bool bgra = false, pending = false;
} color_capture;
void log(void*, const char* message) {
    if (logger) { logger(logger_user, message); return; }
    FILE* file = _wfopen(log_path.c_str(), L"ab");
    if (file) { std::fprintf(file, "%s\n", message); std::fclose(file); }
}
// One completed readback pair, owned by the worker that writes it.
struct ColorPair {
    ComPtr<ID3D12Resource> buffers[2];
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprints[2]{};
    std::wstring root;
    uint64_t generation = 0;
    uint32_t width = 0, height = 0;
    bool bgra = false;
};
void write_colors(std::unique_ptr<ColorPair> pair) {
    for (size_t i = 0; i < 2; ++i) {
        void* mapped = nullptr; if (FAILED(pair->buffers[i]->Map(0, nullptr, &mapped))) continue;
        const auto path = pair->root + L"fg-input-" + std::to_wstring(pair->generation) + (i ? L"-hudless.ppm" : L"-final.ppm");
        FILE* output = _wfopen(path.c_str(), L"wb");
        if (output) {
            std::fprintf(output, "P6\n%u %u\n255\n", pair->width, pair->height);
            std::vector<unsigned char> row(size_t(pair->width) * 3);
            for (uint32_t y = 0; y < pair->height; ++y) {
                auto* pixels = static_cast<unsigned char*>(mapped) + pair->footprints[i].Offset + size_t(y) * pair->footprints[i].Footprint.RowPitch;
                for (uint32_t x = 0; x < pair->width; ++x) for (uint32_t c = 0; c < 3; ++c)
                    row[size_t(x) * 3 + c] = pixels[size_t(x) * 4 + (pair->bgra ? 2 - c : c)];
                std::fwrite(row.data(), 1, row.size(), output);
            }
            std::fclose(output);
        }
        pair->buffers[i]->Unmap(0, nullptr);
    }
    log(nullptr, "FG completed-colour and HUD-less input pair saved.");
}
void capture_colors(ID3D12GraphicsCommandList* list, ID3D12Resource* final, ID3D12Resource* hudless, uint64_t frame) {
    if (log_path.empty()) return;
    auto& capture = color_capture;
    if (capture.pending && capture.value && capture.completion && capture.completion->GetCompletedValue() >= capture.value) {
        // Two full-resolution files: a worker writes them so Present does not wait on the disk.
        auto pair = std::make_unique<ColorPair>();
        for (size_t i = 0; i < 2; ++i) { pair->buffers[i] = std::move(capture.buffers[i]); pair->footprints[i] = capture.footprints[i]; }
        pair->root = log_path.substr(0, log_path.find_last_of(L"/\\") + 1);
        pair->generation = capture.generation; pair->width = capture.width; pair->height = capture.height; pair->bgra = capture.bgra;
        capture.pending = false;
        try { std::thread(write_colors, std::move(pair)).detach(); } catch (...) {}
    }
    if (capture.pending || capture.generation == status_generation || status_samples < 30 || !hudless) return;
    const auto desc = final->GetDesc(), guide = hudless->GetDesc();
    if (desc.Format != DXGI_FORMAT_R8G8B8A8_UNORM && desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM) return;
    if (guide.Format != desc.Format || guide.Width != desc.Width || guide.Height != desc.Height) return;
    ComPtr<ID3D12Device> device; if (FAILED(final->GetDevice(IID_PPV_ARGS(&device)))) return;
    ID3D12Resource* sources[]{final, hudless};
    for (size_t i = 0; i < 2; ++i) {
        uint64_t bytes = 0; device->GetCopyableFootprints(&desc, 0, 1, 0, &capture.footprints[i], nullptr, nullptr, &bytes);
        D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_READBACK;
        D3D12_RESOURCE_DESC storage{}; storage.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; storage.Width = bytes;
        storage.Height = storage.DepthOrArraySize = storage.MipLevels = 1; storage.SampleDesc.Count = 1;
        storage.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        if (FAILED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &storage, D3D12_RESOURCE_STATE_COPY_DEST, nullptr,
            IID_PPV_ARGS(&capture.buffers[i])))) return;
        const auto state_before = i ? D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE : D3D12_RESOURCE_STATE_PRESENT;
        rsf::transition(list, sources[i], state_before, D3D12_RESOURCE_STATE_COPY_SOURCE);
        D3D12_TEXTURE_COPY_LOCATION source{}; source.pResource = sources[i]; source.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        D3D12_TEXTURE_COPY_LOCATION target{}; target.pResource = capture.buffers[i].Get(); target.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
        target.PlacedFootprint = capture.footprints[i]; list->CopyTextureRegion(&target, 0, 0, 0, &source, nullptr);
        rsf::transition(list, sources[i], D3D12_RESOURCE_STATE_COPY_SOURCE, state_before);
    }
    capture.frame = frame; capture.generation = status_generation; capture.width = static_cast<uint32_t>(desc.Width);
    capture.height = desc.Height; capture.bgra = desc.Format == DXGI_FORMAT_B8G8R8A8_UNORM;
    capture.value = 0; capture.pending = true;
}
bool finished(Slot& slot) {
    if (slot.unknown) {
        if (slot.unknown_generation == rsf_d3d11_present_generation()) return false;
        slot.unknown = false;
    }
    if (!slot.value) return true;
    if (!slot.completion) return false;
    const auto done = slot.completion->GetCompletedValue();
    return done != UINT64_MAX && done >= slot.value;
}
bool copy(ID3D12GraphicsCommandList* list, ID3D12Resource* source, ComPtr<ID3D12Resource>& target,
    D3D12_RESOURCE_STATES source_state = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
    D3D12_RESOURCE_STATES* target_state = nullptr) {
    if (!source) return false;
    const auto desc = source->GetDesc();
    if (!target || target->GetDesc().Width != desc.Width || target->GetDesc().Height != desc.Height || target->GetDesc().Format != desc.Format) {
        ComPtr<ID3D12Device> device; if (FAILED(list->GetDevice(IID_PPV_ARGS(&device)))) return false;
        auto storage = desc; storage.Flags = D3D12_RESOURCE_FLAG_NONE;
        D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
        target.Reset();
        if (FAILED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &storage,
            D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&target)))) return false;
        if (target_state) *target_state = D3D12_RESOURCE_STATE_COMMON;
    }
    rsf::copy_transitioned(list, target.Get(), target_state ? *target_state : D3D12_RESOURCE_STATE_COMMON,
        D3D12_RESOURCE_STATE_COMMON, source, source_state, source_state);
    if (target_state) *target_state = D3D12_RESOURCE_STATE_COMMON;
    return true;
}
void prepare(void*, void* swapchain, void*, void* commands, void* buffer, rsf_streamline_host*, void* context, uint32_t) {
    std::lock_guard<std::mutex> lock(guard);
    ComPtr<IDXGISwapChain3> chain;
    Window identity{};
    if (swapchain && SUCCEEDED(static_cast<IUnknown*>(swapchain)->QueryInterface(IID_PPV_ARGS(&chain)))) {
        const auto index = chain->GetCurrentBackBufferIndex();
        if (index < windows.size()) { identity = windows[index]; windows[index] = {}; }
    }
    window_frame = identity.frame; window_session = identity.session; window_view = identity.view;
    auto& timing = cpu[window_frame % cpu.size()];
    window_generation = timing.id == window_frame ? timing.generation : 0;
    // Present has advanced beyond these unpresented CPU frames. Release their shared
    // tokens even when Unity skipped a camera or input preparation refused that frame.
    if (auto* host = rsf_d3d11_present_host())
        for (auto& older : cpu) if (older.id && older.id < window_frame && !older.completed) {
            rsf_streamline_host_abort(host, older.id); older.completed = true;
        }
    const auto* provider = rsf_d3d11_present_provider();
    if (!provider || !context) { state.vendor = {}; state.last_result = 0; state.reason = RSF_NATIVE_FG_REASON_NONE; consumed = nullptr; return; }
    rsf_fg_status capability{}; capability.struct_size = sizeof(capability);
    if (provider->status(context, &capability) != RSF_BACKEND_OK) return;
    if (status_generation != rsf_d3d11_present_generation()) {
        status_generation = rsf_d3d11_present_generation(); status_samples = prepare_failures = 0;
    }
    auto& slot = slots[window_frame % slots.size()];
    const bool captured = window_frame && slot.ready && slot.submitted && slot.record.frame_id == window_frame &&
        slot.record.session_id == window_session && slot.record.view_id == window_view;
    const bool cpu_valid = timing.id == window_frame && timing.generation == rsf_d3d11_present_generation() &&
        timing.begun && timing.simulation && !timing.failed;
    const bool hudless = ui_mode == RSF_UI_MODE_NONE || (slot.hudless_ready && slot.hudless_submitted);
    // The plugin decides through the record's screen class or RSF_FRAME_FLAG_NO_FG.
    const bool reset = (slot.record.flags & RSF_FRAME_FLAG_RESET) != 0;
    const bool allowed = rsf_frame_allows_fg(&slot.record) != 0;
    const bool valid = captured && cpu_valid && hudless && allowed && !reset;
    rsf_fg_options request{sizeof(request), RSF_FG_ABI_VERSION, valid ? options.mode : RSF_FG_OFF,
        std::min(options.generated_frames, capability.max_generated_frames), 0,
        capability.pacing_owner == RSF_PACING_REFLEX ? options.reflex_mode : RSF_REFLEX_OFF,
        capability.pacing_owner == RSF_PACING_XELL || capability.pacing_owner == RSF_PACING_REFLEX ? options.frame_limit_us : 0u};
    consumed = nullptr;
    state.last_result = provider->configure(context, &request);
    state.vendor = capability; state.source_frame_id = window_frame;
    state.reason = valid ? RSF_NATIVE_FG_REASON_NONE : !captured || !cpu_valid ? RSF_NATIVE_FG_REASON_NOT_PREPARED :
        reset ? RSF_NATIVE_FG_REASON_RESET : !allowed ? RSF_NATIVE_FG_REASON_SCENE_UNSUPPORTED : RSF_NATIVE_FG_REASON_HUDLESS_MISSING;
    if (diagnostics++ < 12 || diagnostics % 600 == 0) {
        char text[384]; std::snprintf(text, sizeof(text), "Unity FG source=%llu captured=%llu view=%u/%u cpu=%u simulation=%u submitted=%u hudless=%u screen=%u allowed=%u generation=%llu/%llu valid=%u configure=%d",
            static_cast<unsigned long long>(window_frame), static_cast<unsigned long long>(slot.record.frame_id), slot.record.view_id, window_view,
            timing.begun, timing.simulation, slot.submitted, slot.hudless_ready, slot.record.screen, allowed, static_cast<unsigned long long>(timing.generation),
            static_cast<unsigned long long>(rsf_d3d11_present_generation()), valid, state.last_result); log(nullptr, text);
    }
    if (state.last_result != RSF_BACKEND_OK || !valid) return;
    auto record = slot.record; record.input_qpc = timing.input; record.phase = RSF_PHASE_UI_COMPLETE;
    rsf_fg_frame frame{}; frame.struct_size = sizeof(frame); frame.record = &record;
    frame.interpolate = options.mode != RSF_FG_OFF; frame.motion_scale_x = frame.motion_scale_y = 1;
    void* textures[]{buffer, slot.depth.Get(), slot.motion.Get()};
    rsf_backend_resource* resources[]{&frame.backbuffer, &frame.depth, &frame.motion};
    auto* list = static_cast<ID3D12GraphicsCommandList*>(commands);
    for (size_t i = 0; i < 3; ++i) {
        auto& r = *resources[i]; r.struct_size = sizeof(r); r.resource = textures[i]; r.generation = record.resource_generation;
        r.width = i ? record.render_width : record.output_width; r.height = i ? record.render_height : record.output_height;
        r.state = i ? D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE : D3D12_RESOURCE_STATE_PRESENT;
        if (i) {
            auto& current = i == 1 ? slot.depth_state : slot.motion_state;
            if (current != D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE)
                rsf::transition(list, static_cast<ID3D12Resource*>(textures[i]), current, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
            current = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        }
    }
    if (ui_mode != RSF_UI_MODE_NONE) {
        auto& r = frame.hudless; r.struct_size = sizeof(r); r.resource = slot.hudless.Get();
        r.generation = record.resource_generation; r.width = record.output_width; r.height = record.output_height;
        r.state = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        if (slot.hudless_state != D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE)
            rsf::transition(list, slot.hudless.Get(), slot.hudless_state, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        slot.hudless_state = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    }
    if (rsf_d3d11_present_backend() == RSF_FG_BACKEND_FSR3)
        capture_colors(list, static_cast<ID3D12Resource*>(buffer), slot.hudless.Get(), record.frame_id);
    state.last_result = provider->prepare(context, commands, &frame);
    // Vendors consume persistent inputs during asynchronous Present. Keep the declared
    // read states until retirement; the next capture transitions them for reuse.
    if (state.last_result != RSF_BACKEND_OK && prepare_failures++ < 8) {
        char text[224]; std::snprintf(text, sizeof(text), "Unity FG prepare backend=%u source=%llu result=%d render=%ux%u output=%ux%u",
            rsf_d3d11_present_backend(), static_cast<unsigned long long>(window_frame), state.last_result,
            record.render_width, record.render_height, record.output_width, record.output_height); log(nullptr, text);
    }
    if (state.last_result == RSF_BACKEND_OK) { consumed = &slot; ++state.tagged_frames; }
    else { request.mode = RSF_FG_OFF; provider->configure(context, &request); }
}
void retire(void*, void* context) {
    std::unique_lock<std::mutex> lock(guard);
    const auto* provider = rsf_d3d11_present_provider();
    if (!provider || !context) { state.vendor = {}; consumed = nullptr; return; }
    rsf_fg_status status{}; status.struct_size = sizeof(status);
    if (provider->status(context, &status) == RSF_BACKEND_OK) {
        state.vendor = status;
        if (++status_samples <= 3 || status_samples % 120 == 0) {
            char text[256]; std::snprintf(text, sizeof(text), "Unity FG status backend=%u source=%llu prepare=%d mode=%u active=%u vendor=%d generated_callbacks=%llu presented=%llu valid_stats=0x%x",
                rsf_d3d11_present_backend(), static_cast<unsigned long long>(window_frame), state.last_result,
                status.effective_mode, status.active, status.vendor_status, static_cast<unsigned long long>(status.generated_callbacks),
                static_cast<unsigned long long>(status.total_presented), status.valid_statistics); log(nullptr, text);
        }
    }
    // A frame without tags (FG off, an ineligible screen) still joins while slots are held, so
    // capture never waits on a retirement that only a generated frame would bring.
    const bool held = std::any_of(slots.begin(), slots.end(), [](const Slot& slot) { return slot.unknown; });
    if (!consumed && !held) return;
    // Vendor retirement can wait for asynchronous presentation. Let main-thread CPU events
    // proceed while it runs; they may be needed to service the window thread.
    lock.unlock();
    // The bridge joins the vendor's retirement on the provider queue and signals a fence that
    // also covers the source capture, the preparation list and earlier queue work.
    void* fence = nullptr; uint64_t value = 0;
    const auto result = rsf_d3d11_present_retire(context, nullptr, &fence, &value);
    lock.lock();
    Slot* slot = consumed; consumed = nullptr;
    if (slot) slot->ready = false;
    if (result != RSF_BACKEND_OK || !fence) {
        if (!slot) return;
        // Nothing was signalled. Hold the slot rather than wait on a value that never arrives.
        slot->completion.Reset(); slot->value = 0;
        slot->unknown = true; slot->unknown_generation = rsf_d3d11_present_generation();
        return;
    }
    ComPtr<ID3D12Fence> joined = static_cast<ID3D12Fence*>(fence);
    if (slot) { slot->completion = joined; slot->value = value; }
    // This join also covers earlier provider-queue reads, so it releases slots whose own retirement failed.
    for (auto& other : slots) if (other.unknown) { other.completion = joined; other.value = value; other.unknown = false; }
    if (slot && color_capture.pending && color_capture.frame == slot->record.frame_id) {
        color_capture.completion = joined; color_capture.value = value;
    }
}
void latency(void*, const rsf_observer_present_event* event) {
    if (!window_frame || event->flags & DXGI_PRESENT_TEST) return;
    const auto generation = window_generation;
    if (!event->completed) {
        rsf_d3d11_present_marker(generation, window_frame, RSF_LATENCY_RENDER_SUBMIT_END, 0);
        rsf_d3d11_present_marker(generation, window_frame, RSF_LATENCY_PRESENT_START, 0);
    } else {
        rsf_d3d11_present_marker(generation, window_frame, RSF_LATENCY_PRESENT_END, 0);
        if (auto* host = rsf_d3d11_present_host()) rsf_streamline_host_abort(host, window_frame);
        std::lock_guard<std::mutex> lock(guard);
        auto& timing = cpu[window_frame % cpu.size()];
        if (timing.id == window_frame) timing.completed = true;
    }
}
int accept(void*, void* hwnd) {
    DWORD process = 0; wchar_t name[64]{}; GetWindowThreadProcessId(static_cast<HWND>(hwnd), &process);
    return process == GetCurrentProcessId() && GetClassNameW(static_cast<HWND>(hwnd), name, 64) &&
        std::wcscmp(name, L"UnityWndClass") == 0;
}
// [UnitySR] SDK folder setting for a generation backend, as UTF-8. Empty when unset.
std::string backend_directory(const wchar_t* ini, uint32_t backend) {
    const auto* info = rsf_fg_backend_info_for(backend);
    if (!info) return {};
    const std::wstring key(info->ini_key, info->ini_key + std::strlen(info->ini_key));
    return rsf::unity_config::utf8(rsf::unity_config::path_setting(ini, key.c_str()));
}
}
extern "C" RSF_RUNTIME_API rsf_backend_result rsf_fg12_install(const rsf_fg12_setup* request) try {
    if (!request || request->struct_size < sizeof(*request) || !request->runtime_directory_utf8 || !request->accept_window ||
        request->options.struct_size < sizeof(request->options) || request->options.mode > RSF_FG_FIXED ||
        !request->options.generated_frames || request->units_to_meters <= 0)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (installed) return RSF_BACKEND_ERROR_NEEDS_RESTART;
    if ((request->backend == 0 && !request->runtime_switching) ||
        (request->backend != 0 && !rsf_fg_backend_known(request->backend) &&
         !(request->backend == RSF_FG_BACKEND_AUTO && request->runtime_switching)))
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    directory = request->runtime_directory_utf8; options = request->options;
    options.reflex_mode = RSF_REFLEX_OFF; logger = request->log; logger_user = request->user;
    rsf_d3d11_present_setup setup{}; setup.struct_size = sizeof(setup); setup.runtime_directory_utf8 = directory.c_str();
    setup.backend = request->backend; setup.max_generated_frames = request->max_generated_frames;
    setup.depth_inverted = request->depth_inverted; setup.depth_infinite = request->depth_infinite;
    setup.units_to_meters = request->units_to_meters;
    setup.runtime_switching = request->runtime_switching;
    setup.ui_mode = request->ui_mode ? request->ui_mode : RSF_UI_MODE_NONE; ui_mode = setup.ui_mode;
    setup.fsr3_directory_utf8 = request->fsr3_directory_utf8; setup.fsr4_directory_utf8 = request->fsr4_directory_utf8;
    setup.xess_directory_utf8 = request->xess_directory_utf8;
    setup.streamline_directory_utf8 = request->streamline_directory_utf8;
    setup.engine_type = request->engine_type; setup.engine_version_utf8 = request->engine_version_utf8;
    setup.project_id_utf8 = request->project_id_utf8;
    setup.log = log; setup.user = request->user; setup.accept_window = request->accept_window;
    setup.prepare = prepare; setup.retire = retire; setup.latency_event = latency;
    installed = rsf_d3d11_present_install(&setup) != 0;
    return installed ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
}
catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }
extern "C" RSF_RUNTIME_API uint32_t __stdcall rsf_unity_fg_start(const wchar_t* ini) try {
    if (!ini || installed) return installed ? 0u : 1u;
    const auto backend = rsf::unity_config::fg_choice_start(ini);
    // Off still installs the switching facade, which loads from the FSR3 folder.
    directory = backend_directory(ini, backend && backend != RSF_FG_BACKEND_AUTO ? backend : RSF_FG_BACKEND_FSR3);
    if (directory.empty()) return 3;
    log_path = rsf::unity_config::path_setting(ini, L"Log");
    if (GetPrivateProfileIntW(L"UnitySR", L"GraphicsDebug", 0, ini)) {
        ComPtr<ID3D12Debug> layer;
        if (SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&layer)))) {
            layer->EnableDebugLayer(); log(nullptr, "Unity D3D12 debug layer enabled before graphics creation.");
        } else log(nullptr, "Unity D3D12 debug layer unavailable.");
    }
    options.mode = RSF_FG_FIXED; options.generated_frames = GetPrivateProfileIntW(L"UnitySR", L"GeneratedFrames", 1, ini);
    if (!options.generated_frames || options.generated_frames > 15) return 4;
    const auto limit = GetPrivateProfileIntW(L"UnitySR", L"FrameLimitFPS", 0, ini);
    options.frame_limit_us = limit ? 1000000u / limit : 0;
    rsf_fg12_setup setup{}; setup.struct_size = sizeof(setup);
    setup.backend = backend; setup.max_generated_frames = UINT32_MAX;
    setup.options = options; setup.depth_inverted = 1; setup.units_to_meters = 1; setup.accept_window = accept;
    setup.runtime_switching = 1; setup.ui_mode = RSF_UI_MODE_BACKBUFFER_HUDLESS;
    setup.engine_type = RSF_DLSS_ENGINE_UNITY; setup.engine_version_utf8 = RSF_UNITY_ENGINE_VERSION;
    setup.project_id_utf8 = RSF_UNITY_PROJECT_ID;
    const auto fsr3_path = backend_directory(ini, RSF_FG_BACKEND_FSR3), fsr4_path = backend_directory(ini, RSF_FG_BACKEND_FSR4);
    const auto xess_path = backend_directory(ini, RSF_FG_BACKEND_XESS), streamline_path = backend_directory(ini, RSF_FG_BACKEND_DLSS);
    setup.fsr3_directory_utf8 = fsr3_path.c_str(); setup.fsr4_directory_utf8 = fsr4_path.c_str(); setup.xess_directory_utf8 = xess_path.c_str();
    setup.streamline_directory_utf8 = streamline_path.c_str();
    const std::string stable_directory = directory; setup.runtime_directory_utf8 = stable_directory.c_str();
    installed = rsf_fg12_install(&setup) == RSF_BACKEND_OK;
    log(nullptr, installed ? "Shared FSR/XeSS generation installed before Unity graphics creation." : "Generation interception refused; engine presentation retained.");
    return installed ? 0u : 5u;
}
catch (...) { return 5; }
extern "C" RSF_RUNTIME_API void rsf_fg12_cpu(const rsf_game_cpu_event* event) {
    if (!event || !event->source_frame_id || !rsf_d3d11_present_has_owner()) return;
    rsf_fg_choice_resolve_auto(rsf_d3d11_present_backend());
    // Sleep is a CPU operation. Do not retain a lock needed by the presenting thread.
    uint64_t generation = rsf_d3d11_present_generation();
    const auto acquired = event->stage == RSF_GAME_CPU_FRAME_BEGIN ? rsf_d3d11_present_acquire(event->source_frame_id) : RSF_BACKEND_OK;
    const auto slept = event->stage == RSF_GAME_CPU_PACING ? rsf_d3d11_present_begin(event->source_frame_id, &generation) : RSF_BACKEND_OK;
    std::lock_guard<std::mutex> lock(guard);
    auto& frame = cpu[event->source_frame_id % cpu.size()];
    if (event->stage == RSF_GAME_CPU_FRAME_BEGIN) { frame = {}; frame.id = event->source_frame_id; frame.failed = acquired != RSF_BACKEND_OK; }
    if (frame.id != event->source_frame_id) return;
    if (event->stage == RSF_GAME_CPU_PACING) { frame.generation = generation; frame.begun = slept == RSF_BACKEND_OK; frame.failed |= !frame.begun; }
    if (event->stage == RSF_GAME_CPU_INPUT_SAMPLE) {
        frame.input = event->timestamp_qpc;
        frame.failed |= rsf_d3d11_present_marker(frame.generation, frame.id, RSF_LATENCY_SIMULATION_START, 0) != RSF_BACKEND_OK;
    }
    if (event->stage == RSF_GAME_CPU_SIMULATION_END) {
        frame.simulation = true;
        frame.failed |= rsf_d3d11_present_marker(frame.generation, frame.id, RSF_LATENCY_SIMULATION_END, 0) != RSF_BACKEND_OK;
    }
}
extern "C" RSF_RUNTIME_API void rsf_fg12_capture(void* commands, const rsf_frame_record* record, void* depth, void* motion) {
    if (!commands || !record || !record->frame_id || !rsf_d3d11_present_has_owner()) return;
    std::lock_guard<std::mutex> lock(guard);
    auto& slot = slots[record->frame_id % slots.size()];
    if (!finished(slot)) return;
    if (slot.ready && slot.record.frame_id == record->frame_id) {
        // Multiple primary views/evaluations require an explicit output association. Refuse
        // this source frame instead of allowing the latest capture to win.
        slot.record.flags |= RSF_FRAME_FLAG_RESET; return;
    }
    slot.ready = false; slot.submitted = false; slot.hudless_ready = false; slot.hudless_submitted = false;
    auto* list = static_cast<ID3D12GraphicsCommandList*>(commands);
    if (!copy(list, static_cast<ID3D12Resource*>(depth), slot.depth, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, &slot.depth_state) ||
        !copy(list, static_cast<ID3D12Resource*>(motion), slot.motion, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, &slot.motion_state)) return;
    slot.record = *record; slot.ready = true;
    auto& timing = cpu[record->frame_id % cpu.size()];
    if (timing.id == record->frame_id) {
        timing.render_started = true;
        timing.failed |= rsf_d3d11_present_marker(timing.generation, record->frame_id, RSF_LATENCY_RENDER_SUBMIT_START, 0) != RSF_BACKEND_OK;
    }
}
extern "C" RSF_RUNTIME_API void rsf_fg12_hudless(void* commands, const rsf_game_render_pass* pass) {
    if (!commands || !pass || !pass->color_input || !pass->source_frame_id || !rsf_d3d11_present_has_owner()) return;
    std::lock_guard<std::mutex> lock(guard);
    auto& slot = slots[pass->source_frame_id % slots.size()];
    if (!slot.ready || slot.record.frame_id != pass->source_frame_id || slot.record.session_id != pass->session_id ||
        slot.record.view_id != static_cast<uint32_t>(pass->view_key)) return;
    auto* source = static_cast<ID3D12Resource*>(pass->color_input);
    const auto desc = source->GetDesc();
    if (desc.Width != slot.record.output_width || desc.Height != slot.record.output_height) return;
    slot.hudless_ready = copy(static_cast<ID3D12GraphicsCommandList*>(commands), source, slot.hudless, D3D12_RESOURCE_STATE_RENDER_TARGET, &slot.hudless_state);
}
extern "C" RSF_RUNTIME_API void rsf_fg12_window(const rsf_game_render_pass* pass) {
    if (!pass || !rsf_d3d11_present_is_owner(pass->swapchain)) return;
    ComPtr<IDXGISwapChain3> chain;
    if (FAILED(static_cast<IUnknown*>(pass->swapchain)->QueryInterface(IID_PPV_ARGS(&chain)))) return;
    const auto index = chain->GetCurrentBackBufferIndex();
    if (index >= windows.size()) return;
    std::lock_guard<std::mutex> lock(guard);
    windows[index] = {pass->source_frame_id, pass->session_id, static_cast<uint32_t>(pass->view_key)};
    auto& timing = cpu[pass->source_frame_id % cpu.size()];
    if (timing.id == pass->source_frame_id && !timing.render_started) {
        timing.render_started = true;
        timing.failed |= rsf_d3d11_present_marker(timing.generation, timing.id, RSF_LATENCY_RENDER_SUBMIT_START, 0) != RSF_BACKEND_OK;
    }
}
extern "C" RSF_RUNTIME_API void rsf_fg12_submitted(uint64_t session, uint64_t id) {
    if (!id) return;
    std::lock_guard<std::mutex> lock(guard);
    auto& slot = slots[id % slots.size()];
    if (slot.ready && slot.record.frame_id == id && slot.record.session_id == session) {
        slot.submitted = true;
        if (slot.hudless_ready) slot.hudless_submitted = true;
    }
}
extern "C" RSF_RUNTIME_API int rsf_fg12_status(rsf_native_fg_status* output) {
    if (!output || output->struct_size < sizeof(*output)) return 0;
    std::lock_guard<std::mutex> lock(guard); *output = state; output->struct_size = sizeof(*output);
    output->requested = options; output->available = rsf_d3d11_present_has_owner() && rsf_d3d11_present_backend() != 0; return 1;
}
extern "C" RSF_RUNTIME_API void rsf_fg12_options(const rsf_native_fg_options* request) {
    if (!request || request->struct_size < sizeof(*request) || request->mode > RSF_FG_FIXED ||
        request->reflex_mode > RSF_REFLEX_BOOST || !request->generated_frames) return;
    std::lock_guard<std::mutex> lock(guard); options = *request;
}
