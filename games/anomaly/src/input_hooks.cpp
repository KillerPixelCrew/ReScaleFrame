// SPDX-License-Identifier: MIT
// Native leaves of the CInput::OnFrame input-fact hook. Rust owns the vtable slot, admission, the in-flight state,
// the polling thread and receipt routing. The detour reads the engine frame and calls the original.
#include "input_hooks.h"
#include "engine_slots.h"
// Engine-fact sinks forwarded to fg_stage.rs; no graphics or vendor SDK dependency here.
namespace rsf::anomaly::fg { void input_begin(uint32_t) noexcept; void input_received(uint32_t,uint32_t) noexcept; }
extern "C" {
bool rsf_anomaly_input_frame_enter() noexcept;
uint32_t rsf_anomaly_input_polling_begin() noexcept;
void rsf_anomaly_input_frame_exit(uint32_t ticket) noexcept;
bool rsf_anomaly_input_idle() noexcept;
uint32_t rsf_anomaly_input_route(uint8_t kind) noexcept;
}
namespace rsf::anomaly::input {
namespace {
using Frame = void(__fastcall*)(void*);
void __fastcall on_frame(void* self) {
    struct Flight { uint32_t ticket{}; ~Flight() { rsf_anomaly_input_frame_exit(ticket); } } flight;
    if (rsf_anomaly_input_frame_enter()) {
        uint32_t frame{};
        if (guarded_copy(&frame, rsf_anomaly_input_slot_counter(), sizeof(frame))) {
            fg::input_begin(frame);
            flight.ticket = rsf_anomaly_input_polling_begin();
        }
    }
    reinterpret_cast<Frame>(rsf_anomaly_slot_original(RSF_ANOMALY_INPUT_SLOT))(self);
}
}
void received(overlay::DeviceKind kind) noexcept {
    if (const auto kinds = rsf_anomaly_input_route(static_cast<uint8_t>(kind))) fg::input_received(kinds,0);
}
bool install(std::byte* image, uint32_t size, const BuildProfile& build, const char** reason) noexcept {
    const auto checked = guard_image(image, size);
    const auto& p = *build.input;
    for (const auto* site : {&p.frame, &p.keyboard, &p.mouse}) {
        if (!matches(checked, *site)) {
            *reason = site->name; return false;
        }
    }
    if (!rsf_anomaly_guard_hex(&checked, p.frame.rva, p.frame_code)) {
        *reason = "CInput OnFrame complete body/vtable guard"; return false;
    }
    if (!rsf_anomaly_slot_install(RSF_ANOMALY_INPUT_SLOT, &checked, p.frame_vtable_rva, p.frame.rva,
        build.native->device_frame_rva, reinterpret_cast<void*>(&on_frame), compare_slot)) {
        *reason = "CInput primary slot 0"; return false;
    }
    return true;
}
bool idle() noexcept { return rsf_anomaly_input_idle(); }
bool enable() noexcept { return rsf_anomaly_slot_set(RSF_ANOMALY_INPUT_SLOT, true); }
bool disable() noexcept { return rsf_anomaly_slot_set(RSF_ANOMALY_INPUT_SLOT, false); }
}
