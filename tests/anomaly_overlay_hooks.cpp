// SPDX-License-Identifier: MIT
#include "native_hook_ops.h"
#define DIRECTINPUT_VERSION 0x0800
#include <dinput.h>
#include <cstddef>

// Keep SDK constants tied to the Rust gate's device record layout.
static_assert(sizeof(DIDEVICEOBJECTDATA) == 24 && offsetof(DIDEVICEOBJECTDATA, dwTimeStamp) == 8);
static_assert(DIMOFS_BUTTON0 == 12 && DIMOFS_BUTTON7 == 19 && DIGDD_PEEK == 1 && DISCL_EXCLUSIVE == 1);
static_assert(DIERR_INPUTLOST == HRESULT(0x8007001EL) && DIERR_NOTACQUIRED == HRESULT(0x8007000CL));

extern "C" int rsf_anomaly_overlay_hooks_fixture(const rsf::anomaly::HookOps*, const DIDATAFORMAT*) noexcept;

int main() {
    return rsf_anomaly_overlay_hooks_fixture(&rsf::anomaly::hook_ops, &c_dfDIMouse);
}
