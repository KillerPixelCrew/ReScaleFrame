// SPDX-License-Identifier: MIT
// MinHook calls, PE header reads and the guarded copy for the Rust hook-set owner (native_hooks.rs).
#include "native_hooks.h"
#include "native_memory.h"
#include <MinHook.h>

_Static_assert(sizeof(IMAGE_NT_HEADERS64) == 264 && IMAGE_DOS_SIGNATURE == 0x5a4d &&
    IMAGE_NT_SIGNATURE == 0x4550 && IMAGE_FILE_MACHINE_AMD64 == 0x8664 &&
    IMAGE_NT_OPTIONAL_HDR64_MAGIC == 0x20b, "PE64 facts");
/* The Rust owner holds the set as one opaque pointer. */
_Static_assert(sizeof(rsf_anomaly_hook_set) == 8, "hook set layout");
_Static_assert(sizeof(rsf_anomaly_guard_image) == 24 && offsetof(rsf_anomaly_guard_image, read) == 16,
    "guard image layout");

static bool initialize(void) {
    const MH_STATUS status = MH_Initialize();
    return status == MH_OK || status == MH_ERROR_ALREADY_INITIALIZED;
}
static bool create(void* target, void* replacement, void** original) {
    return MH_CreateHook(target, replacement, original) == MH_OK;
}
// A never enabled or queued entry has no disable step.
static void remove_unqueued(void* target) { (void)MH_RemoveHook(target); }
static bool queue(void* target, bool enabled) {
    return (enabled ? MH_QueueEnableHook(target) : MH_QueueDisableHook(target)) == MH_OK;
}
static bool apply(void) { return MH_ApplyQueued() == MH_OK; }

static bool read_mapped(const void* owner, const void* address, void* out, size_t size, bool executable) {
    return rsf_anomaly_hook_read_mapped(owner, address, out, size, executable, guarded_copy);
}

static bool guarded_dos(const void* module, rsf_anomaly_dos_facts* out) {
    return rsf_anomaly_hook_read_dos(module, out, false, guarded_copy);
}
static bool guarded_pe(const void* module, const void* address, rsf_anomaly_pe_facts* out) {
    return rsf_anomaly_hook_read_pe(module, address, out, false, guarded_copy);
}
static bool mapped_dos(const void* module, rsf_anomaly_dos_facts* out) {
    return rsf_anomaly_hook_read_dos(module, out, true, guarded_copy);
}
static bool mapped_pe(const void* module, const void* address, rsf_anomaly_pe_facts* out) {
    return rsf_anomaly_hook_read_pe(module, address, out, true, guarded_copy);
}

const rsf_anomaly_image_headers rsf_anomaly_guarded_image_headers = {guarded_dos, guarded_pe};
const rsf_anomaly_hook_platform rsf_anomaly_native_hook_platform = {
    {mapped_dos, mapped_pe}, read_mapped, {initialize, create, remove_unqueued, queue, apply},
};

const rsf_anomaly_hook_ops* rsf_anomaly_minhook_ops(void) {
    return &rsf_anomaly_native_hook_platform.hooks;
}

bool rsf_anomaly_hooks_install(rsf_anomaly_hook_set* set, void* module, uint32_t image_size,
    const rsf_anomaly_hook_site* sites, size_t count)
{
    return rsf_anomaly_hook_set_install(&set->batch, &rsf_anomaly_native_hook_platform, module, image_size,
        sites, count);
}
bool rsf_anomaly_hooks_install_resolved(rsf_anomaly_hook_set* set, const rsf_anomaly_hook_target* sites,
    size_t count)
{
    return rsf_anomaly_hook_set_install_resolved(&set->batch, &rsf_anomaly_native_hook_platform, sites, count);
}
bool rsf_anomaly_hooks_enable(rsf_anomaly_hook_set* set) {
    return rsf_anomaly_hook_set_set_enabled(&set->batch, &rsf_anomaly_native_hook_platform, true);
}
bool rsf_anomaly_hooks_disable(rsf_anomaly_hook_set* set) {
    return rsf_anomaly_hook_set_set_enabled(&set->batch, &rsf_anomaly_native_hook_platform, false);
}
bool rsf_anomaly_hooks_created(const rsf_anomaly_hook_set* set) {
    return rsf_anomaly_hook_set_created(&set->batch);
}
