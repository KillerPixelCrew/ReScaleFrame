// SPDX-License-Identifier: MIT
#include "native_fg_cpu_platform.h"
#include "input_receipts.h"
#include "latency_antilag.h"
#include <rescaleframe/game_api.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <algorithm>
#include <array>
#include <cstdio>

extern "C" int32_t rsf_d3d11_present_pace_for_generation(uint64_t, rsf_pacing_owner, uint64_t);

namespace {
using Microsoft::WRL::ComPtr;

static_assert(RSF_NFG_CPU_NOT_READY == RSF_BACKEND_ERROR_NOT_READY);
static_assert(RSF_NFG_CPU_INVALID == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
static_assert(RSF_NFG_CPU_INVALID == RSF_ERROR_INVALID_ARGUMENT);
static_assert(RSF_NFG_CPU_ABI_MISMATCH == RSF_ERROR_ABI_MISMATCH);
static_assert(RSF_NFG_CPU_SIM_START == RSF_LATENCY_SIMULATION_START);
static_assert(RSF_NFG_CPU_SIM_END == RSF_LATENCY_SIMULATION_END);
static_assert(RSF_NFG_CPU_SUBMIT_START == RSF_LATENCY_RENDER_SUBMIT_START);
static_assert(RSF_NFG_CPU_SUBMIT_END == RSF_LATENCY_RENDER_SUBMIT_END);
static_assert(RSF_NFG_CPU_PRESENT_START == RSF_LATENCY_PRESENT_START);
static_assert(RSF_NFG_CPU_PRESENT_END == RSF_LATENCY_PRESENT_END);

size_t describe_address(uint64_t address, char* text, size_t size) {
    HMODULE module = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            reinterpret_cast<LPCWSTR>(uintptr_t(address)), &module) || !module) return 0;
    MEMORY_BASIC_INFORMATION region{};
    if (!VirtualQuery(reinterpret_cast<void*>(uintptr_t(address)), &region, sizeof(region)) ||
        !(region.Protect & (PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY))) return 0;
    wchar_t path[MAX_PATH]{};
    const auto length = GetModuleFileNameW(module, path, MAX_PATH);
    if (!length || length >= MAX_PATH) return 0;
    const wchar_t* leaf = path; for (const wchar_t* c = path; *c; ++c) if (*c == L'\\') leaf = c + 1;
    const int written = std::snprintf(text, size, " %ls+0x%llx", leaf,
        static_cast<unsigned long long>(address - uint64_t(uintptr_t(module))));
    return written > 0 && size_t(written) < size ? size_t(written) : 0;
}
void* duplicate_render_thread() {
    HANDLE copy = nullptr;
    return DuplicateHandle(GetCurrentProcess(), GetCurrentThread(), GetCurrentProcess(), &copy,
        THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION, FALSE, 0) ? copy : nullptr;
}

size_t sample_render_thread(void* thread, uint64_t* addresses, size_t capacity) {
    if (capacity == 0 || SuspendThread(thread) == DWORD(-1)) return 0;
    CONTEXT context{};
    context.ContextFlags = CONTEXT_CONTROL;
    size_t count = 0;
    if (GetThreadContext(thread, &context)) {
        addresses[count++] = context.Rip;
        const auto stack = reinterpret_cast<const void*>(uintptr_t(context.Rsp));
        MEMORY_BASIC_INFORMATION region{};
        if (VirtualQuery(stack, &region, sizeof(region)) &&
            region.State == MEM_COMMIT && !(region.Protect & (PAGE_NOACCESS | PAGE_GUARD))) {
            const auto offset = uintptr_t(stack) - uintptr_t(region.BaseAddress);
            if (offset < region.RegionSize) {
                const size_t words = std::min(capacity - count, (region.RegionSize - offset) / sizeof(uint64_t));
                SIZE_T copied = 0;
                if (words && ReadProcessMemory(GetCurrentProcess(), stack, addresses + count, words * sizeof(uint64_t), &copied))
                    count += copied / sizeof(uint64_t);
            }
        }
    }
    // No module lookup, formatting or log sink runs while the thread is suspended.
    return ResumeThread(thread) == DWORD(-1) ? 0 : count;
}

HANDLE completion_event() {
    static const HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    return event;
}

int wait_completion(uint32_t timeout, uint32_t sent_messages) {
    const auto event = completion_event();
    if (!event) return 0;
    if (!sent_messages) return WaitForSingleObject(event, timeout) != WAIT_FAILED;
    const auto result = MsgWaitForMultipleObjectsEx(1, &event, timeout, QS_SENDMESSAGE, 0);
    if (result == WAIT_OBJECT_0 + 1) {
        MSG message{};
        PeekMessageW(&message, nullptr, 0, 0, PM_NOREMOVE | PM_QS_SENDMESSAGE);
    }
    return result != WAIT_FAILED;
}

uint32_t owned_window_thread(uint64_t key) {
    DWORD process{};
    const auto thread = GetWindowThreadProcessId(reinterpret_cast<HWND>(uintptr_t(key)), &process);
    return process == GetCurrentProcessId() ? thread : 0;
}

uint32_t adapter_vendor(void* device) {
    ComPtr<IDXGIFactory4> factory;
    ComPtr<IDXGIAdapter1> adapter;
    DXGI_ADAPTER_DESC1 desc{};
    if (SUCCEEDED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) &&
        SUCCEEDED(factory->EnumAdapterByLuid(static_cast<ID3D12Device*>(device)->GetAdapterLuid(), IID_PPV_ARGS(&adapter))) &&
        SUCCEEDED(adapter->GetDesc1(&desc)) && !(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) {
        return desc.VendorId;
    }
    return 0;
}

uint64_t receipt_owner(const rsf_nfg_receipt_candidate* owners, size_t count,
    uint64_t session, uint64_t generation, uint64_t timestamp) {
    std::array<rsf::latency::ReceiptOwner, RSF_NFG_CPU_CAPACITY> copied{};
    if (count > copied.size()) return 0;
    for (size_t i = 0; i < count; ++i) {
        const auto& row = owners[i];
        copied[i] = {row.source, row.session, row.generation, row.ready, row.simulation_begin, row.eligible != 0};
    }
    return rsf::latency::receipt_owner(std::span(copied.data(), count), session, generation, timestamp);
}

rsf_streamline_host* host(void* value) { return static_cast<rsf_streamline_host*>(value); }
} // namespace

extern "C" const rsf_nfg_cpu_platform rsf_native_fg_cpu_platform{
    rsf_d3d11_present_has_owner,
    rsf_d3d11_present_generation,
    rsf_d3d11_present_backend,
    rsf_d3d11_present_status_get,
    +[]() -> void* {
        rsf_streamline_graphics graphics{sizeof(graphics)};
        return rsf_d3d11_present_graphics(&graphics) ? graphics.native_device : nullptr;
    },
    adapter_vendor,
    +[]() -> void* { return rsf_d3d11_present_host(); },
    +[](void* sdk, rsf_streamline_latency_status* status) { return rsf_streamline_host_latency_status(host(sdk), status); },
    +[](void* sdk, uint32_t mode, uint32_t interval) { return rsf_streamline_host_reflex(host(sdk), mode, interval); },
    +[](void* sdk, uint64_t source) { return rsf_streamline_host_token(host(sdk), source); },
    +[](void* sdk, uint64_t source) { return rsf_streamline_host_presented(host(sdk), source); },
    +[](void* sdk, uint64_t source) { return rsf_streamline_host_abort(host(sdk), source); },
    +[](void* sdk, uint64_t through) { return rsf_streamline_host_retire_through(host(sdk), through); },
    +[](void* sdk) { return rsf_streamline_host_reset_sources(host(sdk)); },
    rsf_d3d11_present_acquire,
    rsf_d3d11_present_pace_for_generation,
    rsf_d3d11_present_marker,
    rsf_d3d11_present_input,
    rsf_d3d11_present_receipt,
    rsf_d3d11_present_abort,
    +[]() -> void* {
        static auto* driver = rsf::latency::make_antilag_driver().release();
        return rsf::latency::AntiLag::create_context(*driver);
    },
    receipt_owner,
    owned_window_thread,
    +[](void* window, uint32_t message) { return int(PostMessageW(static_cast<HWND>(window), message, 0, 0)); },
    +[]() { if (const auto event = completion_event()) SetEvent(event); },
    wait_completion,
    +[]() -> uint32_t {
        LASTINPUTINFO input{sizeof(input), 0};
        return GetLastInputInfo(&input) ? uint32_t(GetTickCount() - input.dwTime) : UINT32_MAX;
    },
    +[]() -> uint64_t { return GetTickCount64(); },
    +[]() -> uint64_t {
        LARGE_INTEGER frequency{};
        QueryPerformanceFrequency(&frequency);
        return uint64_t(frequency.QuadPart);
    },
    +[]() -> uint32_t { return GetCurrentThreadId(); },
    rsf_frame_allows_fg,
    duplicate_render_thread,
    sample_render_thread,
    describe_address,
};

extern "C" RSF_RUNTIME_API void rsf_native_fg_rendered_source_set(uint32_t enabled) {
    rsf_native_cpu_rendered_source_set(enabled);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_reflex_ordering_set(uint32_t threaded) {
    rsf_core_native_fg_reflex_ordering_set(threaded);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_stage(const char* stage, uint64_t source, uint64_t frame, int32_t result) {
    rsf_core_native_fg_stage(stage, source, frame, result, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_cpu(const rsf_game_cpu_event* event) {
    rsf_core_native_fg_cpu(event, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_frame(const rsf_game_render_pass* pass, uint32_t begin) {
    rsf_core_native_fg_frame(pass, begin, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_fg_submission(const rsf_game_render_pass* pass, uint32_t begin) {
    rsf_core_native_fg_submission(pass, begin, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_graphics(uint32_t api) {
    rsf_core_native_latency_graphics(api);
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_render(uint64_t source, uint32_t begin) {
    rsf_core_native_latency_render(source, begin, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_present(uint64_t source, void* chain, uint32_t begin) {
    rsf_core_native_latency_present(source, chain, begin, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API int rsf_native_latency_status_get(rsf_native_latency_status* out) {
    return rsf_core_native_latency_status_get(out, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API int rsf_native_latency_frame_get(rsf_native_latency_frame* out) {
    return rsf_core_native_latency_frame_get(out);
}
extern "C" RSF_RUNTIME_API int32_t rsf_native_latency_window_bind(const rsf_native_latency_window* binding) {
    return rsf_core_native_latency_window_bind(binding, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API int rsf_native_input_routes_get(rsf_native_input_routes* out) {
    return rsf_core_native_input_routes_get(out);
}
extern "C" RSF_RUNTIME_API int rsf_native_latency_ping(void* window) {
    return rsf_core_native_latency_ping(window, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_retire(uint64_t session, uint64_t through) {
    rsf_core_native_latency_retire(session, through, &rsf_native_fg_cpu_platform);
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_end_session(uint64_t session) {
    rsf_core_native_latency_end_session(session, &rsf_native_fg_cpu_platform);
}
