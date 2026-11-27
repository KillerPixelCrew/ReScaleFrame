// SPDX-License-Identifier: MIT
#include <windows.h>
#define DIRECTINPUT_VERSION 0x0800
#include <dinput.h>
#include <stdbool.h>
#include <stddef.h>

// Match the device records and flags consumed by the adapter's Rust gate.
_Static_assert(sizeof(DIDEVICEOBJECTDATA) == 24 && offsetof(DIDEVICEOBJECTDATA, dwTimeStamp) == 8,
    "DirectInput buffered record");
_Static_assert(DIMOFS_BUTTON0 == 12 && DIMOFS_BUTTON7 == 19 && DIGDD_PEEK == 1 && DISCL_EXCLUSIVE == 1,
    "DirectInput offsets and flags");
_Static_assert(DIERR_INPUTLOST == (HRESULT)0x8007001eUL && DIERR_NOTACQUIRED == (HRESULT)0x8007000cUL,
    "DirectInput acquisition errors");

int rsf_anomaly_overlay_hooks_fixture(const DIDATAFORMAT*);

int main(void) {
    return rsf_anomaly_overlay_hooks_fixture(&c_dfDIMouse);
}
