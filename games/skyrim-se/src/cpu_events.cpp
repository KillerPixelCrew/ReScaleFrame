// SPDX-License-Identifier: GPL-3.0-only
#include "cpu_events.h"
#include <MinHook.h>
#include <windows.h>
#include <array>
#include <atomic>
#include <cstring>
#include <cstdio>

namespace rsf::skyrim {
namespace {
struct Frame {
    rsf_game_host_services host{};
    uint64_t source{};
    bool sampled{}, render_entered{}, simulation_ended{};
};
thread_local Frame* current{};
std::atomic<uint64_t> next_source{1};
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
    // No primary GPU renderer has been published yet. Source identity alone cannot authorize FG.
    event.render_expected = 0;
    try { frame.host.cpu_event(frame.host.user, &event); } catch (...) {}
}
short tick(void*);
uintptr_t poll(void*);
uintptr_t render(void*);
struct Hook {
    uint32_t rva;
    std::array<unsigned char, 19> bytes;
    void* detour;
    void* original{};
    void* target{};
    bool created{}, enabled{};
};
std::array<Hook, 3> hooks{{
    {0x658870, {0x48,0x8b,0xc4,0x55,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48,0x81,0xec,0x30,0x01,0x00,0x00}, reinterpret_cast<void*>(&tick)},
    {0xcf95f0, {0x48,0x89,0x5c,0x24,0x10,0x48,0x89,0x74,0x24,0x18,0x57,0x48,0x83,0xec,0x30,0x0f,0x29,0x74,0x24}, reinterpret_cast<void*>(&poll)},
    {0x6565b0, {0x40,0x53,0x55,0x56,0x57,0x41,0x54,0x41,0x56,0x41,0x57,0x48,0x81,0xec,0x60,0x01,0x00,0x00,0x48}, reinterpret_cast<void*>(&render)}
}};
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
    if (current) {
        current->render_entered = true;
        if (!current->simulation_ended) {
            current->simulation_ended = true;
            emit(*current, RSF_GAME_CPU_SIMULATION_END);
        }
    }
    return reinterpret_cast<uintptr_t (*)(void*)>(hooks[2].original)(main);
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
}
