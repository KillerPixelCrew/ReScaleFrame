// SPDX-License-Identifier: MIT
// Native leaves of the CInput::OnFrame input-fact hook. Rust owns the vtable slot, admission, the in-flight state,
// the polling thread and receipt routing. The detour reads the engine frame and calls the original.
#include "input_hooks.h"
#include "engine_slots.h"
#include <cstddef>
// Engine-fact sinks forwarded to fg_stage.rs; no graphics or vendor SDK dependency here.
namespace rsf::anomaly::fg { void input_begin(uint32_t) noexcept; void input_received(uint32_t,uint32_t) noexcept; }
extern "C" {
bool rsf_anomaly_input_install(const rsf_anomaly_guard_image*, const rsf::anomaly::InputProfile*,
    uint32_t counter_rva, void* replacement, rsf_anomaly_slot_exchange, const char** reason) noexcept;
uint32_t rsf_anomaly_input_frame_begin(decltype(&rsf::anomaly::guarded_copy), void (*emit)(uint32_t) noexcept) noexcept;
void rsf_anomaly_input_frame_exit(uint32_t ticket) noexcept;
bool rsf_anomaly_input_idle() noexcept;
void rsf_anomaly_input_received(uint8_t kind, void (*emit)(uint32_t, uint32_t) noexcept) noexcept;
}
static_assert(sizeof(rsf::anomaly::InputProfile) == 112 &&
    offsetof(rsf::anomaly::InputProfile, keyboard) == 32 && offsetof(rsf::anomaly::InputProfile, mouse) == 64 &&
    offsetof(rsf::anomaly::InputProfile, frame_vtable_rva) == 96 && offsetof(rsf::anomaly::InputProfile, frame_code) == 104 &&
    RSF_ANOMALY_INPUT_SLOT == 0);
namespace rsf::anomaly::input {
namespace {
using Frame = void(__fastcall*)(void*);
void __fastcall on_frame(void* self) {
    struct Flight { uint32_t ticket; ~Flight() { rsf_anomaly_input_frame_exit(ticket); } }
        flight{rsf_anomaly_input_frame_begin(guarded_copy, fg::input_begin)};
    reinterpret_cast<Frame>(rsf_anomaly_slot_original(RSF_ANOMALY_INPUT_SLOT))(self);
}
}
void received(overlay::DeviceKind kind) noexcept {
    rsf_anomaly_input_received(static_cast<uint8_t>(kind), fg::input_received);
}
bool install(std::byte* image, uint32_t size, const BuildProfile& build, const char** reason) noexcept {
    const auto checked = guard_image(image, size);
    return rsf_anomaly_input_install(&checked, build.input, build.native->device_frame_rva,
        reinterpret_cast<void*>(&on_frame), compare_slot, reason);
}
bool idle() noexcept { return rsf_anomaly_input_idle(); }
bool enable() noexcept { return rsf_anomaly_slot_set(RSF_ANOMALY_INPUT_SLOT, true); }
bool disable() noexcept { return rsf_anomaly_slot_set(RSF_ANOMALY_INPUT_SLOT, false); }
}
