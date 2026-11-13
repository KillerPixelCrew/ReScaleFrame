// SPDX-License-Identifier: GPL-3.0-only
#include "input_hooks.h"
// Engine-fact sinks implemented in fg_stage.cpp; no graphics or vendor SDK dependency here.
namespace rsf::anomaly::fg { void input_begin(uint32_t) noexcept; void input_received(uint32_t,uint32_t) noexcept; }
#include <windows.h>
#include <atomic>
#include <cstring>
namespace rsf::anomaly::input {
namespace {
using Frame = void(__fastcall*)(void*);
Frame original{};
void** slot{};
std::byte* module{};
const BuildProfile* selected{};
std::atomic<bool> active{};
std::atomic<uint32_t> in_flight{};
thread_local bool polling{};
bool bytes(std::byte* p, const char* hex) noexcept {
    const auto digit = [](char c) { return c <= '9' ? c-'0' : c-'a'+10; };
    __try {
        for (size_t i=0; hex[i*2]; ++i)
            if (uint8_t(p[i]) != uint8_t(digit(hex[i*2])*16+digit(hex[i*2+1]))) return false;
        return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) { return false; }
}
void __fastcall on_frame(void* self) {
    in_flight.fetch_add(1,std::memory_order_acq_rel);
    struct Flight { ~Flight() { in_flight.fetch_sub(1,std::memory_order_acq_rel); } } flight;
    if (!active.load(std::memory_order_acquire)) { original(self); return; }
    uint32_t frame{};
    std::memcpy(&frame, module + selected->native->device_frame_rva, sizeof(frame));
    fg::input_begin(frame);
    const bool previous = polling;
    polling = true;
    struct Exit { bool saved; ~Exit() { polling = saved; } } exit{previous};
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
void received(uint32_t kinds) noexcept { if (polling && active.load(std::memory_order_acquire)) fg::input_received(kinds,0); }
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
    module=image; selected=&build; original=reinterpret_cast<Frame>(*candidate); slot=candidate;
    if (!enable()) { *reason = "CInput vtable swap"; return false; }
    return true;
}
bool idle() noexcept { return in_flight.load(std::memory_order_acquire)==0; }
bool enable() noexcept {
    if (!slot) return true;
    if (*slot != reinterpret_cast<void*>(&on_frame) && !exchange(reinterpret_cast<void*>(original), reinterpret_cast<void*>(&on_frame))) return false;
    active.store(true, std::memory_order_release); return true;
}
bool disable() noexcept {
    active.store(false, std::memory_order_release);
    return !slot || *slot == reinterpret_cast<void*>(original) || exchange(reinterpret_cast<void*>(&on_frame), reinterpret_cast<void*>(original));
}
}
