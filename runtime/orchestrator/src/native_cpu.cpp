// SPDX-License-Identifier: MIT
// CPU facts, markers and reservations live in native_cpu.rs. This file keeps the
// process-wide pacing lease and supplies the native receipt replay check.
#include <rescaleframe/native_cpu.h>
#include "input_receipts.h"
#include <windows.h>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <iterator>
#include <mutex>
#include <type_traits>

extern "C" int rsf_core_cpu_dispatch(const rsf_game_cpu_event*, rsf_game_cpu_event_fn, void*,
    int (*replay)(uint64_t* window, uint64_t receipt_id));
namespace {
static_assert(sizeof(rsf::latency::ReceiptWindow) == 2 * sizeof(uint64_t) &&
    std::is_trivially_copyable_v<rsf::latency::ReceiptWindow>);
// The Rust dispatcher owns the window and calls this under its state lock.
int replay(uint64_t* window, uint64_t receipt_id) noexcept {
    rsf::latency::ReceiptWindow state;
    std::memcpy(&state, window, sizeof(state));
    const bool accepted = state.accept(receipt_id);
    std::memcpy(window, &state, sizeof(state));
    return accepted;
}
std::mutex lease_guard;
HANDLE pacing_lease{};
uint32_t pacing_client{};
}
extern "C" RSF_RUNTIME_API int rsf_native_latency_claim(uint32_t client) {
    if (client!=1 && client!=2) return 0;
    std::lock_guard lock(lease_guard);
    if (pacing_lease) return pacing_client==client;
    wchar_t name[96]{};std::swprintf(name,std::size(name),L"Local\\ReScaleFrame.LatencyOwner.%lu",GetCurrentProcessId());
    SetLastError(ERROR_SUCCESS);
    const auto event=CreateEventW(nullptr,TRUE,FALSE,name);
    const auto result=GetLastError();
    if (!event) return 0;
    if (result==ERROR_ALREADY_EXISTS) {CloseHandle(event);return 0;}
    pacing_lease=event;pacing_client=client;return 1;
}
extern "C" RSF_RUNTIME_API int rsf_native_latency_has_claim(uint32_t client) {
    std::lock_guard lock(lease_guard);return pacing_lease && pacing_client==client;
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_release_claim(uint32_t client) {
    std::lock_guard lock(lease_guard);
    if (pacing_lease && pacing_client==client) {CloseHandle(pacing_lease);pacing_lease=nullptr;pacing_client=0;}
}
extern "C" RSF_RUNTIME_API int rsf_native_cpu_dispatch(const rsf_game_cpu_event* incoming,
    rsf_game_cpu_event_fn supplied_sink, void* supplied_user)
{
    return rsf_core_cpu_dispatch(incoming, supplied_sink, supplied_user, replay);
}
extern "C" RSF_RUNTIME_API int rsf_native_cpu_event(const rsf_game_cpu_event* event) {
    return rsf_native_cpu_dispatch(event, nullptr, nullptr);
}
