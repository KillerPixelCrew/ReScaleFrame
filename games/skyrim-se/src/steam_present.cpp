// SPDX-License-Identifier: GPL-3.0-only
#include "steam_present.h"
#include <rescaleframe/steam_d3d11_chain.h>
#include <windows.h>

namespace rsf::skyrim {
namespace {
bool cold_owner() {
    const auto read = [](uintptr_t address, uintptr_t& value) {
        SIZE_T copied{};
        return ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<void*>(address),
            &value, sizeof(value), &copied) && copied == sizeof(value);
    };
    uintptr_t owner{}, device{};
    return read(uintptr_t(GetModuleHandleW(nullptr)) + 0x3330188, owner) &&
        (!owner || (read(owner + 56, device) && !device));
}
}
bool steam_present_prepare(const rsf_game_host_services& host) noexcept {
    return rsf::steam::prepare_d3d11_chain(host, cold_owner);
}
}
