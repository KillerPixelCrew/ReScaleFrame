// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <windows.h>
#include <xinput.h>
#include <MinHook.h>
#include <array>
#include <atomic>

namespace rsf::windows {
// Adapter-side observation of successful public XInput reads. No polling or input injection.
class XInputFacts {
    using Read = DWORD (WINAPI*)(DWORD, XINPUT_STATE*);
    struct Slot { void* target{}; Read original{}; };
    inline static XInputFacts* instance{};
    std::array<Slot, 6> slots{};
    std::atomic<unsigned> readers{};
    std::atomic<bool> enabled{};
    std::atomic<void (*)()> received{};
    template<size_t I> static DWORD WINAPI read(DWORD index, XINPUT_STATE* state) {
        auto& self = *instance;
        ++self.readers;
        const auto result = self.slots[I].original(index, state);
        if (self.enabled.load() && successful(result, index, state)) {
            if (const auto callback = self.received.load()) callback();
        }
        --self.readers;
        return result;
    }
public:
    static bool successful(DWORD result, DWORD index, const XINPUT_STATE* state) {
        return result == ERROR_SUCCESS && index < XUSER_MAX_COUNT && state;
    }
    void install_loaded(void (*callback)()) {
        if (!instance) instance = this;
        received.store(callback); enabled.store(true);
        const std::array<Read, 6> detours{read<0>, read<1>, read<2>, read<3>, read<4>, read<5>};
        for (const auto name : {L"xinput1_4.dll", L"xinput1_3.dll", L"xinput9_1_0.dll"}) {
            const auto module = GetModuleHandleW(name);
            if (!module) continue;
            for (const auto symbol : std::array<LPCSTR, 2>{"XInputGetState", MAKEINTRESOURCEA(100)}) {
                auto* target = reinterpret_cast<void*>(GetProcAddress(module, symbol));
                if (!target) continue;
                bool found = false;
                for (const auto& slot : slots) if (slot.target == target) found = true;
                if (found) continue;
                for (size_t i = 0; i < slots.size(); ++i) if (!slots[i].target) {
                    auto& slot = slots[i];
                    if (MH_CreateHook(target, reinterpret_cast<void*>(detours[i]), reinterpret_cast<void**>(&slot.original)) == MH_OK) {
                        slot.target = target;
                        if (MH_EnableHook(target) != MH_OK) { MH_RemoveHook(target); slot = {}; }
                    }
                    break;
                }
            }
        }
    }
    void disable() {
        enabled.store(false);
        for (const auto& slot : slots) if (slot.target) MH_DisableHook(slot.target);
    }
    bool idle() const { return readers.load() == 0; }
    bool remove() {
        disable();
        if (!idle()) return false;
        for (auto& slot : slots) if (slot.target) { MH_RemoveHook(slot.target); slot = {}; }
        return true;
    }
};
}
