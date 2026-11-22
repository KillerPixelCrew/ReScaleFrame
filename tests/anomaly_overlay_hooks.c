// SPDX-License-Identifier: MIT
#include <MinHook.h>
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

// C projection of the Rust HookOps table. MinHook remains linked by this host.
typedef struct rsf_fixture_hook_ops {
    bool (*initialize)(void);
    bool (*create)(void*, void*, void**);
    void (*remove_unqueued)(void*);
    bool (*queue)(void*, bool);
    bool (*apply)(void);
} rsf_fixture_hook_ops;

_Static_assert(sizeof(rsf_fixture_hook_ops) == 40 &&
    offsetof(rsf_fixture_hook_ops, create) == 8 &&
    offsetof(rsf_fixture_hook_ops, remove_unqueued) == 16 &&
    offsetof(rsf_fixture_hook_ops, queue) == 24 &&
    offsetof(rsf_fixture_hook_ops, apply) == 32, "HookOps layout");

static bool initialize(void) {
    const MH_STATUS status = MH_Initialize();
    return status == MH_OK || status == MH_ERROR_ALREADY_INITIALIZED;
}
static bool create(void* target, void* replacement, void** original) {
    return MH_CreateHook(target, replacement, original) == MH_OK;
}
static void remove_unqueued(void* target) { (void)MH_RemoveHook(target); }
static bool queue(void* target, bool enable) {
    return (enable ? MH_QueueEnableHook(target) : MH_QueueDisableHook(target)) == MH_OK;
}
static bool apply(void) { return MH_ApplyQueued() == MH_OK; }

int rsf_anomaly_overlay_hooks_fixture(const rsf_fixture_hook_ops*, const DIDATAFORMAT*);

int main(void) {
    const rsf_fixture_hook_ops hooks = {initialize, create, remove_unqueued, queue, apply};
    return rsf_anomaly_overlay_hooks_fixture(&hooks, &c_dfDIMouse);
}
