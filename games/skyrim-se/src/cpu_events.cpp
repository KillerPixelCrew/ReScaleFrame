// SPDX-License-Identifier: GPL-3.0-only
#include "cpu_events.h"
#include <MinHook.h>
#include <windows.h>
#include <array>
#include <atomic>
#include <cstring>
#include <cstdio>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

namespace rsf::skyrim {
namespace {
struct Frame {
    rsf_game_host_services host{};
    uint64_t source{};
    uint64_t render_scope{};
    uint32_t primary_count{};
    bool sampled{}, render_entered{}, simulation_ended{};
};
thread_local Frame* current{};
std::atomic<uint64_t> next_source{1};
std::atomic<uint64_t> next_render_scope{1};
std::atomic<uint32_t> entered{};
std::atomic<bool> active{};
std::atomic<uint64_t> observed_tick{};
std::atomic<uint32_t> poll_samples{};
rsf_game_host_services services{};
uint64_t frequency{};
struct Entry { Entry() { ++entered; } ~Entry() { --entered; } };
void emit(Frame& frame, uint32_t stage)
{
    if (!frame.host.cpu_event) return;
    LARGE_INTEGER now{}; QueryPerformanceCounter(&now);
    rsf_game_cpu_event event{};
    event.struct_size = sizeof(event); event.stage = stage; event.session_id = frame.host.session_id;
    event.source_frame_id = frame.source; event.timestamp_qpc = uint64_t(now.QuadPart);
    event.qpc_frequency = frequency; event.event_version = RSF_GAME_CPU_EVENT_VERSION;
    event.flags = RSF_GAME_CPU_INPUT_KEYED;
    if (stage == RSF_GAME_CPU_FRAME_END && !frame.render_entered) event.flags |= RSF_GAME_CPU_RENDER_SKIPPED;
    event.render_expected = frame.primary_count;
    try { frame.host.cpu_event(frame.host.user, &event); } catch (...) {}
}
short tick(void*);
uintptr_t poll(void*);
uintptr_t render(void*);
uintptr_t world(void*, uint32_t, uint8_t);
int dispatch(void*, void*);
struct Hook {
    uint32_t rva;
    std::array<unsigned char, 19> bytes;
    void* detour;
    void* original{};
    void* target{};
    bool created{}, enabled{};
};
std::array<Hook, 5> hooks{{
    {0x658870, {0x48,0x8b,0xc4,0x55,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48,0x81,0xec,0x30,0x01,0x00,0x00}, reinterpret_cast<void*>(&tick)},
    {0xcf95f0, {0x48,0x89,0x5c,0x24,0x10,0x48,0x89,0x74,0x24,0x18,0x57,0x48,0x83,0xec,0x30,0x0f,0x29,0x74,0x24}, reinterpret_cast<void*>(&poll)},
    {0x6565b0, {0x40,0x53,0x55,0x56,0x57,0x41,0x54,0x41,0x56,0x41,0x57,0x48,0x81,0xec,0x60,0x01,0x00,0x00,0x48}, reinterpret_cast<void*>(&render)},
    {0xcfa650, {0x48,0x8b,0xc4,0x48,0x89,0x50,0x10,0x55,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48}, reinterpret_cast<void*>(&dispatch)},
    {0x656e60, {0x48,0x8b,0xc4,0x89,0x50,0x10,0x55,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48,0x8d}, reinterpret_cast<void*>(&world)}
}};
bool copy_input(void* out, const void* pointer, size_t size) noexcept {
#if defined(_MSC_VER)
    __try { std::memcpy(out, pointer, size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION memory{};
    const auto address = uintptr_t(pointer);
    if (!VirtualQuery(pointer, &memory, sizeof(memory)) || memory.State != MEM_COMMIT ||
        (memory.Protect & (PAGE_NOACCESS | PAGE_GUARD)) || address + size < address ||
        address + size > uintptr_t(memory.BaseAddress) + memory.RegionSize) return false;
    std::memcpy(out, pointer, size); return true;
#endif
}
rsf_game_render_pass render_fact(uint32_t role) {
    rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass); pass.role = role;
    if (!current || !current->host.render_pass || !cpu_render_identity(pass)) return pass;
    pass.session_id = current->host.session_id;
    pass.flags |= RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_NO_FG;
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    uintptr_t window{}, root{}, camera{};
    (void)copy_input(&pass.native_frame, reinterpret_cast<const void*>(base + 0x33363f4), sizeof(uint32_t));
    if (copy_input(&root, reinterpret_cast<const void*>(base + 0x3230740), sizeof(root)) && root)
        (void)copy_input(&camera, reinterpret_cast<const void*>(root + 296), sizeof(camera));
    pass.family_key = pass.view_key = pass.history_key = camera;
    if (copy_input(&window, reinterpret_cast<const void*>(base + 0x3330198), sizeof(window)) && window) {
        pass.viewport_key = pass.window_key = pass.rhi_viewport_key = window;
        (void)copy_input(&pass.swapchain, reinterpret_cast<const void*>(window + 0x18), sizeof(pass.swapchain));
    }
    return pass;
}
void report_render(const rsf_game_render_pass& pass, uint32_t begin) {
    if (current && current->host.render_pass && pass.source_frame_id) {
        try { current->host.render_pass(current->host.user, nullptr, &pass, begin); } catch (...) {}
    }
}
uintptr_t world(void* main, uint32_t output, uint8_t flags) {
    Entry lease;
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    auto pass = render_fact(RSF_GAME_RENDER_SUBMISSION);
    const bool primary = caller == uintptr_t(GetModuleHandleW(nullptr)) + 0x656e32 && output == 0 &&
        pass.source_frame_id && pass.view_key && pass.swapchain;
    if (primary) report_render(pass, 1);
    const auto result = reinterpret_cast<uintptr_t (*)(void*, uint32_t, uint8_t)>(hooks[4].original)(main, output, flags);
    if (primary) { ++current->primary_count; report_render(pass, 0); }
    return result;
}
uint32_t input_kinds(const void* list) noexcept {
    struct Event { uintptr_t vtable; uint32_t device, type; uintptr_t next; };
    static_assert(sizeof(Event) == 0x18);
    uintptr_t next{};
    if (!list || !copy_input(&next, list, sizeof(next))) return 0;
    uint32_t kinds{};
    // Native events are borrowed until PollDevices clears its queue after dispatch.
    // Keep only kinds, never buttons, text, deltas or native event pointers.
    for (unsigned count = 0; next && count < 128; ++count) {
        Event event{};
        if (!copy_input(&event, reinterpret_cast<const void*>(next), sizeof(event))) return 0;
        if (event.type == 0) {
            if (event.device == 0 || event.device == 3) kinds |= RSF_GAME_INPUT_KEYBOARD;
            else if (event.device == 1) kinds |= RSF_GAME_INPUT_MOUSE;
            else if (event.device == 2) kinds |= RSF_GAME_INPUT_CONTROLLER;
        } else if (event.type == 1 && event.device == 1) kinds |= RSF_GAME_INPUT_MOUSE;
        else if (event.type == 2 && (event.device == 0 || event.device == 3)) kinds |= RSF_GAME_INPUT_KEYBOARD;
        else if (event.type == 3 && event.device == 2) kinds |= RSF_GAME_INPUT_CONTROLLER;
        next = event.next;
    }
    return next ? 0 : kinds; // Cyclic or excessive lists do not publish partial activity.
}
int dispatch(void* manager, void* list) {
    Entry lease;
    rsf_game_cpu_event event{};
    rsf_game_host_services host{};
    const auto base = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));
    uintptr_t native_manager{};
    if (active.load(std::memory_order_acquire) && copy_input(&native_manager, base + 0x3204b30, sizeof(native_manager)) &&
        uintptr_t(manager) == native_manager) {
        event.input_kind = input_kinds(list);
        if (event.input_kind) {
            host = services;
            LARGE_INTEGER now{}; QueryPerformanceCounter(&now);
            event.struct_size = sizeof(event); event.stage = RSF_GAME_CPU_INPUT_EVENT;
            event.session_id = host.session_id; event.timestamp_qpc = uint64_t(now.QuadPart);
            event.qpc_frequency = frequency; event.event_version = RSF_GAME_CPU_EVENT_VERSION;
            event.flags = RSF_GAME_CPU_INPUT_UNFRAMED;
        }
    }
    const auto result = reinterpret_cast<int (*)(void*, void*)>(hooks[3].original)(manager, list);
    // Worker polling is not assigned the latest tick. The runtime owns any bounded
    // association of this delivered input with an engine source and vendor policy.
    if (event.input_kind && host.cpu_event) {
        try { host.cpu_event(host.user, &event); } catch (...) {}
    }
    return result;
}
bool disable_hooks()
{
    bool disabled = true;
    for (auto& hook : hooks) if (hook.enabled) {
        const auto result = MH_DisableHook(hook.target);
        if (result == MH_OK || result == MH_ERROR_DISABLED) hook.enabled = false;
        else disabled = false;
    }
    return disabled;
}
short tick(void* main)
{
    Entry lease;
    const auto original = reinterpret_cast<short (*)(void*)>(hooks[0].original);
    if (!active.load(std::memory_order_acquire) || current) return original(main);
    Frame frame{}; frame.host = services; frame.source = next_source.fetch_add(1);
    current = &frame;
    observed_tick.store(frame.source, std::memory_order_release);
    emit(frame, RSF_GAME_CPU_FRAME_BEGIN);
    const auto result = original(main);
    emit(frame, RSF_GAME_CPU_FRAME_END);
    observed_tick.store(0, std::memory_order_release);
    current = nullptr;
    return result;
}
uintptr_t poll(void* manager)
{
    Entry lease;
    if (active.load(std::memory_order_acquire) && poll_samples.fetch_add(1) < 16 && services.log) {
        uint32_t native_tick{};
        const auto* base = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));
        std::memcpy(&native_tick, base + 0x33363f8, sizeof(native_tick));
        char message[224]{};
        std::snprintf(message, sizeof(message),
            "Skyrim native poll thread=%lu scoped_source=%llu observed_tick=%llu native_counter=%u; cross-thread observations do not assign ownership",
            GetCurrentThreadId(), static_cast<unsigned long long>(current ? current->source : 0),
            static_cast<unsigned long long>(observed_tick.load(std::memory_order_acquire)), native_tick);
        try { services.log(services.user, message); } catch (...) {}
    }
    if (current && !current->sampled) {
        current->sampled = true;
        emit(*current, RSF_GAME_CPU_INPUT_SAMPLE);
    }
    return reinterpret_cast<uintptr_t (*)(void*)>(hooks[1].original)(manager);
}
uintptr_t render(void* main)
{
    Entry lease;
    const auto previous_scope = current ? current->render_scope : 0;
    if (current) {
        current->render_scope = next_render_scope.fetch_add(1);
        current->render_entered = true;
        if (!current->simulation_ended) {
            current->simulation_ended = true;
            emit(*current, RSF_GAME_CPU_SIMULATION_END);
        }
    }
    const auto pass = render_fact(RSF_GAME_RENDER_FRAME);
    report_render(pass, 1);
    const auto result = reinterpret_cast<uintptr_t (*)(void*)>(hooks[2].original)(main);
    report_render(pass, 0);
    if (current) current->render_scope = previous_scope;
    return result;
}
}

bool cpu_prepare(const rsf_game_host_services& host)
{
    if (active.load() || entered.load()) return false;
    auto* base = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr));
    for (auto& hook : hooks) {
        hook.target = base + hook.rva;
        if (std::memcmp(hook.target, hook.bytes.data(), hook.bytes.size())) return false;
    }
    for (auto& hook : hooks) {
        if (hook.created) continue;
        if (MH_CreateHook(hook.target, hook.detour, &hook.original) != MH_OK) {
            for (auto& previous : hooks) if (previous.created) {
                MH_RemoveHook(previous.target); previous.created = false; previous.original = nullptr;
            }
            return false;
        }
        hook.created = true;
    }
    LARGE_INTEGER value{};
    if (!QueryPerformanceFrequency(&value) || value.QuadPart <= 0) return false;
    frequency = uint64_t(value.QuadPart); services = host;
    return true;
}
bool cpu_start()
{
    for (auto& hook : hooks) {
        if (!hook.created || MH_EnableHook(hook.target) != MH_OK) {
            // A failed start leaves preparation and its borrowed services valid for retry.
            (void)disable_hooks(); return false;
        }
        hook.enabled = true;
    }
    active.store(true, std::memory_order_release); return true;
}
void cpu_quiesce() { active.store(false, std::memory_order_release); }
bool cpu_stop()
{
    cpu_quiesce();
    const bool disabled = disable_hooks();
    if (!disabled || entered.load(std::memory_order_acquire)) return false;
    services = {}; return true;
}
bool cpu_busy() { return entered.load(std::memory_order_acquire) != 0; }
uint64_t cpu_source() { return current ? current->source : 0; }
bool cpu_render_identity(rsf_game_render_pass& pass) {
    if (!current || !current->render_scope || !current->simulation_ended) return false;
    pass.source_frame_id = current->source;
    pass.submission_id = pass.scope_id = current->render_scope;
    pass.flags |= RSF_GAME_RENDER_AFTER_SIMULATION;
    return true;
}
uint32_t cpu_input_kinds(const void* list) noexcept { return input_kinds(list); }
}
