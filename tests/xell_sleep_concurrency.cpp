// SPDX-License-Identifier: GPL-3.0-only
// COM and the vendor call table are the native leaves of the Rust fixture.
#include "../runtime/backends/common/generation12_checks.h"

extern "C" {
int rsf_xell_fixture_run(void);
int32_t rsf_xell_fixture_sleep_mode(uint32_t interval);
int32_t rsf_xell_fixture_sleep(uint32_t sdk_id);
int32_t rsf_xell_fixture_marker(uint32_t sdk_id, uint32_t kind);
int32_t rsf_xell_fixture_enabled(void);
}

namespace {
static_assert(RSF_LATENCY_PRESENT_END == 5);

struct Object final : IUnknown {
    ULONG refs = 1;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID, void**) override { return E_NOINTERFACE; }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++refs; }
    ULONG STDMETHODCALLTYPE Release() override { return --refs; }
} device, queue, chain, fence;

const rsf_gen12_checks checks{
    [](const rsf_generation_setup*, void** owner, void** present) {
        *owner = *present = nullptr;
        return RSF_BACKEND_OK;
    },
    rsf::fg_frame,
    rsf::fg_command,
    []() -> uint32_t { return 0; },
    // The Rust owner takes and retires these real module references. The stub
    // resolver never reads exports from them and no vendor DLL is loaded.
    [](const rsf_generation_setup*, const uint16_t*) -> void* { return LoadLibraryW(L"kernel32.dll"); },
};

const rsf_xeg12_ops ops{
    &checks,
    [](const rsf_generation_setup*, void*, void*, void** api) {
        *api = &device;
        return RSF_BACKEND_OK;
    },
    [](void*) {},
    [](void*, void*, void** context) { *context = &device; return 0; },
    [](void*, void*, void** context) { *context = &device; return 0; },
    [](void*, void*) { return 0; },
    [](void*, void*) { return 0; },
    [](void*, void*, uint32_t* maximum) { *maximum = 2; return 0; },
    [](void*, void*, void*) { return 0; },
    [](void*, rsf_xeg12_version*) {},
    [](void*, void*, const rsf_generation_setup*, uint32_t, void** out) {
        chain.AddRef();
        *out = static_cast<IUnknown*>(&chain);
        return RSF_BACKEND_OK;
    },
    [](void*, void*, uint32_t) { return 0; },
    [](void*, void*, uint32_t) { return 0; },
    [](void*, void*, uint32_t) { return rsf_xell_fixture_enabled(); },
    [](void*, void*, uint32_t interval, uint32_t) { return rsf_xell_fixture_sleep_mode(interval); },
    [](void*, void*, uint32_t sdk_id) { return rsf_xell_fixture_sleep(sdk_id); },
    [](void*, void*, uint32_t sdk_id, rsf_latency_marker kind) { return rsf_xell_fixture_marker(sdk_id, kind); },
    [](void*, void*, uint32_t) { return 0; },
    [](void*, void*, void*, uint32_t, uint32_t, const rsf_backend_resource*) { return 0; },
    [](void*, void*, uint32_t, const rsf_fg_frame*, uint32_t) { return 0; },
    [](void*, void*, rsf_xeg12_present*) { return 0; },
    [](void*) -> void* { fence.AddRef(); return static_cast<IUnknown*>(&fence); },
    []() -> void* { return CreateEventW(nullptr, FALSE, FALSE, nullptr); },
    [](void*, void*, void*, uint64_t) { return 0; },
};
}

extern "C" int32_t rsf_xell_fixture_create(void** session) {
    rsf_generation_setup setup{};
    setup.chain.hwnd = &chain;
    setup.chain.d3d12_device = static_cast<IUnknown*>(&device);
    setup.chain.d3d12_queue = static_cast<IUnknown*>(&queue);
    setup.chain.ui_mode = RSF_UI_MODE_NONE;
    setup.chain.max_generated_frames = 2;
    void* present = nullptr;
    const auto result = rsf_core_xeg12_create(&ops, &setup, session, &present);
    return result == RSF_BACKEND_OK && !present ? RSF_BACKEND_ERROR_NOT_READY : result;
}

extern "C" void rsf_xell_fixture_destroy(void* session) {
    rsf_core_xeg12_provider()->destroy(session);
}

extern "C" int32_t rsf_xell_fixture_configure(void* session, uint32_t frames, uint32_t interval) {
    const rsf_fg_options options{sizeof(options), RSF_FG_ABI_VERSION, RSF_FG_FIXED,
        frames, 0, RSF_REFLEX_ON, interval};
    return rsf_core_xeg12_provider()->configure(session, &options);
}

extern "C" int32_t rsf_xell_fixture_begin(void* session, uint64_t source) {
    return rsf_core_xeg12_provider()->begin_frame(session, source);
}

extern "C" int32_t rsf_xell_fixture_present_end(void* session, uint64_t source) {
    return rsf_core_xeg12_provider()->marker(session, RSF_LATENCY_PRESENT_END, source, 0);
}

int main() { return rsf_xell_fixture_run(); }
