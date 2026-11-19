// SPDX-License-Identifier: MIT
// The CPU dispatcher owns the facts and supplies storage for the native
// receipt replay check.
#include <rescaleframe/native_cpu.h>
#include "input_receipts.h"
#include <cstring>
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
}
extern "C" RSF_RUNTIME_API int rsf_native_cpu_dispatch(const rsf_game_cpu_event* incoming,
    rsf_game_cpu_event_fn supplied_sink, void* supplied_user)
{
    return rsf_core_cpu_dispatch(incoming, supplied_sink, supplied_user, replay);
}
extern "C" RSF_RUNTIME_API int rsf_native_cpu_event(const rsf_game_cpu_event* event) {
    return rsf_native_cpu_dispatch(event, nullptr, nullptr);
}
