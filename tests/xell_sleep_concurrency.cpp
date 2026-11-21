// SPDX-License-Identifier: GPL-3.0-only
// Exercise the actual Rust provider's begin/marker paths with bounded SDK doubles.
#include "../runtime/backends/xess/src/xess_generation.cpp"
#include <condition_variable>
#include <mutex>
#include <thread>
#include <chrono>
#include <cstdio>

namespace {
struct Object final : IUnknown {
    ULONG refs = 1;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID, void**) override { return E_NOINTERFACE; }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++refs; }
    ULONG STDMETHODCALLTYPE Release() override { return --refs; }
} device, queue, chain, fence;

std::mutex gate;
std::condition_variable changed;
bool entered{}, marked{};
bool fail_enable{};
uint32_t sleep_interval{}, sleep_updates{};

rsf_backend_result setup_ok(const rsf_generation_setup*, void** session, void** present)
{
    *session = nullptr;
    *present = nullptr;
    return RSF_BACKEND_OK;
}

rsf_backend_result open_ok(const rsf_generation_setup*, void** api)
{
    *api = nullptr;
    return RSF_BACKEND_OK;
}

int32_t create_context(void*, void*, void** context)
{
    *context = &device;
    return 0;
}

int32_t count_ok(void*, void*, uint32_t* maximum)
{
    *maximum = 2;
    return 0;
}

int32_t configure_sleep(void*, void*, uint32_t interval, uint32_t)
{
    sleep_interval = interval;
    ++sleep_updates;
    return 0;
}

rsf_backend_result create_test_chain(void*, void*, const rsf_generation_setup*, uint32_t, void** out)
{
    chain.AddRef();
    *out = static_cast<IUnknown*>(&chain);
    return RSF_BACKEND_OK;
}

void* create_test_fence(void*)
{
    fence.AddRef();
    return static_cast<IUnknown*>(&fence);
}

int32_t sleeping(void*, void*, uint32_t sdk_id)
{
    if (sdk_id == 1) return 0;
    if (sdk_id != 2) return -1;
    std::unique_lock lock(gate);
    entered = true;
    changed.notify_all();
    return changed.wait_for(lock, std::chrono::seconds(2), [] { return marked; }) ? 0 : -1;
}

int32_t marking(void*, void*, uint32_t sdk_id, rsf_latency_marker kind)
{
    if (kind != RSF_LATENCY_PRESENT_END) return -1;
    if (sdk_id == 2) return 0;
    if (sdk_id != 1) return -1;
    std::lock_guard lock(gate);
    marked = true;
    changed.notify_all();
    return 0;
}
}

int main()
{
    static const rsf_gen12_checks checks{setup_ok, rsf::fg_frame, rsf::fg_command, []() -> uint32_t { return 0; }};
    static const auto ops = [] {
        auto leaves = native_ops();
        leaves.check = &checks;
        leaves.open = open_ok;
        leaves.close = [](void*) {};
        leaves.create_fg = leaves.create_xell = create_context;
        leaves.destroy_fg = leaves.destroy_xell = [](void*, void*) -> int32_t { return 0; };
        leaves.max_count = count_ok;
        leaves.bind_xell = [](void*, void*, void*) -> int32_t { return 0; };
        leaves.version = [](void*, rsf_xeg12_version*) {};
        leaves.create_chain = create_test_chain;
        leaves.compose = leaves.count = [](void*, void*, uint32_t) -> int32_t { return 0; };
        leaves.enabled = [](void*, void*, uint32_t) -> int32_t { return fail_enable ? -1 : 0; };
        leaves.create_fence = create_test_fence;
        leaves.drain = [](void*, void*, void*, uint64_t) -> int32_t { return 0; };
        leaves.sleep_mode = configure_sleep;
        leaves.sleep = sleeping;
        leaves.marker = marking;
        return leaves;
    }();
    const auto& provider = *rsf_core_xeg12_provider();
    rsf_generation_setup setup{};
    setup.chain.hwnd = &chain;
    setup.chain.d3d12_device = static_cast<IUnknown*>(&device);
    setup.chain.d3d12_queue = static_cast<IUnknown*>(&queue);
    setup.chain.ui_mode = RSF_UI_MODE_NONE;
    setup.chain.max_generated_frames = 2;
    void* session = nullptr;
    void* present = nullptr;
    if (rsf_core_xeg12_create(&ops, &setup, &session, &present) != RSF_BACKEND_OK) return 1;
    const std::unique_ptr<void, decltype(provider.destroy)> retire(session, provider.destroy);
    rsf_fg_options options{sizeof(options), RSF_FG_ABI_VERSION, RSF_FG_FIXED, 1, 0, RSF_REFLEX_ON, 0};
    if (!present || provider.configure(session, &options) != RSF_BACKEND_OK ||
        provider.begin_frame(session, 10) != RSF_BACKEND_OK) return 1;

    rsf_backend_result result = RSF_BACKEND_ERROR_NOT_READY;
    std::thread worker([&] { result = provider.begin_frame(session, 11); });
    bool started = false;
    {
        std::unique_lock lock(gate);
        started = changed.wait_for(lock, std::chrono::seconds(2), [] { return entered; });
    }
    if (!started) { worker.join(); return 1; }
    const auto premature = provider.marker(session, RSF_LATENCY_PRESENT_END, 11, 0);
    options.frame_limit_us = 1000;
    const auto busy = provider.configure(session, &options);
    options.frame_limit_us = 0;
    options.generated_frames = 2;
    const auto unchanged_sleep = provider.configure(session, &options);
    const auto old = provider.marker(session, RSF_LATENCY_PRESENT_END, 10, 0);
    worker.join();

    if (old != RSF_BACKEND_OK || result != RSF_BACKEND_OK ||
        provider.marker(session, RSF_LATENCY_PRESENT_END, 11, 0) != RSF_BACKEND_OK) return 2;
    if (provider.begin_frame(session, 11) != RSF_BACKEND_ERROR_INVALID_ARGUMENT) return 3;
    if (premature != RSF_BACKEND_ERROR_NOT_READY || busy != RSF_BACKEND_ERROR_NOT_READY) return 4;
    if (unchanged_sleep != RSF_BACKEND_OK || sleep_updates != 1) return 5;
    // XeLL can accept a new limit before the XeFG update fails. Reapplying
    // the last accepted request must undo the SDK's partially applied limit.
    options.frame_limit_us = 2000;
    fail_enable = true;
    if (provider.configure(session, &options) != RSF_BACKEND_ERROR_FEATURE_FAILED ||
        sleep_interval != 2000 || sleep_updates != 2) return 6;
    options.frame_limit_us = 0;
    fail_enable = false;
    if (provider.configure(session, &options) != RSF_BACKEND_OK ||
        sleep_interval != 0 || sleep_updates != 3) return 7;
    std::puts("PASS prior-frame XeLL marker progresses during next-frame sleep; no graphics device created");
    return 0;
}
