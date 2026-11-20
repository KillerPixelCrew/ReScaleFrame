// SPDX-License-Identifier: MIT
// Native leaves of the CInput::OnFrame input-fact hook: the guarded vtable slot edit, the detour that reads the engine
// frame and calls the original, and the FG sink calls. Admission, the in-flight and retiring state, the polling thread
// and received-event routing are in the adapter's Rust library (input.rs).
#include "input_hooks.h"
// Engine-fact sinks implemented in fg_stage.cpp; no graphics or vendor SDK dependency here.
namespace rsf::anomaly::fg { void input_begin(uint32_t) noexcept; void input_received(uint32_t,uint32_t) noexcept; }
#include <windows.h>
#include <cstring>
extern "C" {
uint32_t rsf_anomaly_input_frame_enter() noexcept;
void rsf_anomaly_input_frame_exit(uint32_t ticket) noexcept;
void rsf_anomaly_input_activate(bool active) noexcept;
bool rsf_anomaly_input_idle() noexcept;
uint32_t rsf_anomaly_input_route(uint8_t kind) noexcept;
}
namespace rsf::anomaly::input {
namespace {
using Frame = void(__fastcall*)(void*);
constexpr uint32_t admitted = 1;  // rsf_anomaly_input_frame_enter ticket bit
Frame original{};
void** slot{};
const std::byte* frame_counter{};
bool bytes(std::byte* p, const char* hex) noexcept {
    const auto digit = [](char c) { return c <= '9' ? c-'0' : c-'a'+10; };
    __try {
        for (size_t i=0; hex[i*2]; ++i)
            if (uint8_t(p[i]) != uint8_t(digit(hex[i*2])*16+digit(hex[i*2+1]))) return false;
        return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) { return false; }
}
void __fastcall on_frame(void* self) {
    struct Flight { uint32_t ticket; ~Flight() { rsf_anomaly_input_frame_exit(ticket); } } flight{rsf_anomaly_input_frame_enter()};
    if (flight.ticket & admitted) {
        uint32_t frame{};
        std::memcpy(&frame, frame_counter, sizeof(frame));
        fg::input_begin(frame);
    }
    original(self);
}
bool exchange(void* expected, void* value) noexcept {
    DWORD old{};
    if (!slot || !VirtualProtect(slot, sizeof(void*), PAGE_READWRITE, &old)) return false;
    const bool replaced = InterlockedCompareExchangePointer(slot, value, expected) == expected;
    DWORD ignored{};
    const bool protected_again = VirtualProtect(slot, sizeof(void*), old, &ignored) != FALSE;
    return replaced && protected_again;
}
}
void received(overlay::DeviceKind kind) noexcept {
    if (const auto kinds = rsf_anomaly_input_route(static_cast<uint8_t>(kind))) fg::input_received(kinds,0);
}
bool install(std::byte* image, uint32_t size, const BuildProfile& build, const char** reason) noexcept {
    if (slot) return enable();
    const auto& p = *build.input;
    for (const auto* site : {&p.frame, &p.keyboard, &p.mouse}) {
        if (site->rva > size || site->expected.size() > size-site->rva ||
            std::memcmp(image+site->rva, site->expected.data(), site->expected.size())) {
            *reason = site->name; return false;
        }
    }
    const auto length = std::strlen(p.frame_code)/2;
    if (p.frame.rva > size || length > size-p.frame.rva || !bytes(image+p.frame.rva, p.frame_code) ||
        p.frame_vtable_rva > size || sizeof(void*) > size-p.frame_vtable_rva) {
        *reason = "CInput OnFrame complete body/vtable guard"; return false;
    }
    auto** candidate = reinterpret_cast<void**>(image+p.frame_vtable_rva);
    if (*candidate != image+p.frame.rva) { *reason = "CInput primary slot 0"; return false; }
    frame_counter=image+build.native->device_frame_rva; original=reinterpret_cast<Frame>(*candidate); slot=candidate;
    if (!enable()) { *reason = "CInput vtable swap"; return false; }
    return true;
}
bool idle() noexcept { return rsf_anomaly_input_idle(); }
bool enable() noexcept {
    if (!slot) return true;
    if (*slot != reinterpret_cast<void*>(&on_frame) && !exchange(reinterpret_cast<void*>(original), reinterpret_cast<void*>(&on_frame))) return false;
    rsf_anomaly_input_activate(true); return true;
}
bool disable() noexcept {
    rsf_anomaly_input_activate(false);
    return !slot || *slot == reinterpret_cast<void*>(original) || exchange(reinterpret_cast<void*>(&on_frame), reinterpret_cast<void*>(original));
}
}
