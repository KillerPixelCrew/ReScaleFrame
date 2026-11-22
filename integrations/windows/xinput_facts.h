// SPDX-License-Identifier: MIT
#pragma once
#include <windows.h>
#include <xinput.h>

extern "C" {
// One owner per adapter DLL. Install after initializing MinHook. The callback must
// return without throwing and remain callable until the detour readers drain.
void rsf_xinput_facts_install_loaded(void (*callback)());
void rsf_xinput_facts_disable();
bool rsf_xinput_facts_idle();
bool rsf_xinput_facts_remove();
}

namespace rsf::windows {
class XInputFacts {
public:
    static bool successful(DWORD result, DWORD index, const XINPUT_STATE* state) {
        return result == ERROR_SUCCESS && index < XUSER_MAX_COUNT && state;
    }
    void install_loaded(void (*callback)()) { rsf_xinput_facts_install_loaded(callback); }
    void disable() { rsf_xinput_facts_disable(); }
    bool idle() const { return rsf_xinput_facts_idle(); }
    bool remove() { return rsf_xinput_facts_remove(); }
};
}
