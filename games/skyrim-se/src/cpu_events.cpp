// SPDX-License-Identifier: MIT
#include "cpu_events.h"
#include "cpu_call.h"
#include "screen_classification.h"

#if defined(_MSC_VER)
#include <intrin.h>
#define RSF_SKYRIM_CALLER uintptr_t(_ReturnAddress())
#else
#define RSF_SKYRIM_CALLER uintptr_t(__builtin_return_address(0))
#endif

extern "C" void* rsf_skyrim_cpu_original(uint32_t) noexcept;
extern "C" bool rsf_skyrim_cpu_tick_admissible() noexcept;
extern "C" void rsf_skyrim_cpu_enter() noexcept;
extern "C" void rsf_skyrim_cpu_leave() noexcept;
extern "C" void rsf_skyrim_cpu_before(rsf_skyrim_cpu_hook, rsf_skyrim_cpu_call*, rsf_game_render_pass*) noexcept;
extern "C" void rsf_skyrim_cpu_after(rsf_skyrim_cpu_hook, rsf_skyrim_cpu_call*, rsf_game_render_pass*,
    uintptr_t, bool returned) noexcept;

static_assert(sizeof(rsf_skyrim_cpu_call) == 344 && alignof(rsf_skyrim_cpu_call) == 8);
static_assert(offsetof(rsf_skyrim_cpu_call, storage) == 24);
static_assert(RSF_SKYRIM_CPU_HOOK_COUNT == 13);

namespace {
struct CpuDetours {
    uint32_t struct_size, version;
    void* functions[RSF_SKYRIM_CPU_HOOK_COUNT];
};
static_assert(sizeof(CpuDetours) == 8 + RSF_SKYRIM_CPU_HOOK_COUNT * sizeof(void*));

struct Scope {
    rsf_skyrim_cpu_hook index;
    rsf_skyrim_cpu_call call;
    rsf_game_render_pass* pass;
    uintptr_t result{};
    bool returned{};

    Scope(rsf_skyrim_cpu_hook index, uintptr_t owner = 0, uintptr_t argument = 0,
        uintptr_t caller = 0, rsf_game_render_pass* pass = nullptr) noexcept
        : index(index), call{owner, argument, caller, {}}, pass(pass)
    {
        rsf_skyrim_cpu_enter();
        rsf_skyrim_cpu_before(index, &call, pass);
    }
    ~Scope()
    {
        rsf_skyrim_cpu_after(index, &call, pass, result, returned);
        rsf_skyrim_cpu_leave();
    }
    Scope(const Scope&) = delete;
    Scope& operator=(const Scope&) = delete;

    template<class T> T complete(T value) noexcept {
        result = static_cast<uintptr_t>(value);
        returned = true;
        return value;
    }
};

template<class Function> Function continuation(rsf_skyrim_cpu_hook index) noexcept
{
    return reinterpret_cast<Function>(rsf_skyrim_cpu_original(index));
}

short tick(void* main)
{
    const auto original = continuation<decltype(&tick)>(RSF_SKYRIM_CPU_TICK);
    if (!rsf_skyrim_cpu_tick_admissible()) return original(main);
    Scope scope{RSF_SKYRIM_CPU_TICK, uintptr_t(main)};
    return scope.complete(original(main));
}

// The native timestep is in XMM1.
uintptr_t poll(void* manager, float timestep)
{
    const auto original = continuation<decltype(&poll)>(RSF_SKYRIM_CPU_POLL);
    Scope scope{RSF_SKYRIM_CPU_POLL, uintptr_t(manager)};
    return scope.complete(original(manager, timestep));
}

uintptr_t render(void* main)
{
    rsf_game_render_pass pass{};
    Scope scope{RSF_SKYRIM_CPU_RENDER, uintptr_t(main), 0, 0, &pass};
    return scope.complete(continuation<decltype(&render)>(RSF_SKYRIM_CPU_RENDER)(main));
}

int dispatch(void* manager, void* list)
{
    Scope scope{RSF_SKYRIM_CPU_DISPATCH, uintptr_t(manager), uintptr_t(list)};
    return scope.complete(continuation<decltype(&dispatch)>(RSF_SKYRIM_CPU_DISPATCH)(manager, list));
}

uintptr_t world(void* main, uint32_t output, uint8_t flags)
{
    rsf_game_render_pass pass{};
    Scope scope{RSF_SKYRIM_CPU_WORLD, uintptr_t(main), output, RSF_SKYRIM_CALLER, &pass};
    return scope.complete(continuation<decltype(&world)>(RSF_SKYRIM_CPU_WORLD)(main, output, flags));
}

uintptr_t controls(void* owner, void* list, void* source)
{
    Scope scope{RSF_SKYRIM_CPU_CONTROLS, uintptr_t(owner), uintptr_t(list)};
    // The event-source pointer in R8 is forwarded unchanged.
    return scope.complete(continuation<decltype(&controls)>(RSF_SKYRIM_CPU_CONTROLS)(owner, list, source));
}

uintptr_t camera_update(void* state)
{
    Scope scope{RSF_SKYRIM_CPU_CAMERA, uintptr_t(state)};
    return scope.complete(continuation<decltype(&camera_update)>(RSF_SKYRIM_CPU_CAMERA)(state));
}

uint32_t jobs_submit()
{
    Scope scope{RSF_SKYRIM_CPU_SUBMIT, 0, 0, RSF_SKYRIM_CALLER};
    return scope.complete(continuation<decltype(&jobs_submit)>(RSF_SKYRIM_CPU_SUBMIT)());
}

uint32_t jobs_join()
{
    Scope scope{RSF_SKYRIM_CPU_JOIN, 0, 0, RSF_SKYRIM_CALLER};
    return scope.complete(continuation<decltype(&jobs_join)>(RSF_SKYRIM_CPU_JOIN)());
}

uintptr_t named_poll(void* data)
{
    Scope scope{RSF_SKYRIM_CPU_NAMED_POLL, 0, 0, RSF_SKYRIM_CALLER};
    return scope.complete(continuation<decltype(&named_poll)>(RSF_SKYRIM_CPU_NAMED_POLL)(data));
}

void movement(void* actor, float timestep, void* argument, float* output)
{
    Scope scope{RSF_SKYRIM_CPU_MOVEMENT, uintptr_t(actor)};
    continuation<decltype(&movement)>(RSF_SKYRIM_CPU_MOVEMENT)(actor, timestep, argument, output);
    scope.complete(0);
}

uintptr_t player_update(void* actor, float timestep)
{
    Scope scope{RSF_SKYRIM_CPU_PLAYER_UPDATE, uintptr_t(actor)};
    return scope.complete(continuation<decltype(&player_update)>(RSF_SKYRIM_CPU_PLAYER_UPDATE)(actor, timestep));
}

// TESCamera::Update forwards to the current state vfunc and has a void result.
void player_camera_update(void* camera)
{
    Scope scope{RSF_SKYRIM_CPU_PLAYER_CAMERA, uintptr_t(camera)};
    continuation<decltype(&player_camera_update)>(RSF_SKYRIM_CPU_PLAYER_CAMERA)(camera);
    scope.complete(0);
}

const CpuDetours detours{sizeof(CpuDetours), 1, {
    reinterpret_cast<void*>(&tick),
    reinterpret_cast<void*>(&poll),
    reinterpret_cast<void*>(&render),
    reinterpret_cast<void*>(&dispatch),
    reinterpret_cast<void*>(&world),
    reinterpret_cast<void*>(&controls),
    reinterpret_cast<void*>(&camera_update),
    reinterpret_cast<void*>(&jobs_submit),
    reinterpret_cast<void*>(&jobs_join),
    reinterpret_cast<void*>(&named_poll),
    reinterpret_cast<void*>(&movement),
    reinterpret_cast<void*>(&player_update),
    reinterpret_cast<void*>(&player_camera_update),
}};
}

extern "C" bool rsf_skyrim_cpu_rust_prepare(const CpuDetours*) noexcept;

extern "C" bool rsf_skyrim_cpu_prepare() noexcept
{
    return rsf_skyrim_cpu_rust_prepare(&detours);
}

extern "C" bool rsf_skyrim_query_menus(uintptr_t open_address, uintptr_t frozen_address, uintptr_t ui,
    uintptr_t loading, uintptr_t main, rsf::skyrim::NativeScreenFacts* facts) noexcept try
{
    const auto open = reinterpret_cast<bool (*)(void*, const void*)>(open_address);
    const auto frozen = reinterpret_cast<bool (*)(void*)>(frozen_address);
    auto* owner = reinterpret_cast<void*>(ui);
    facts->loading_menu = open(owner, reinterpret_cast<const void*>(loading));
    facts->main_menu = open(owner, reinterpret_cast<const void*>(main));
    facts->frozen_background = frozen(owner);
    return true;
} catch (...) { return false; }

extern "C" void rsf_skyrim_call_cpu_event(
    const rsf_game_host_services* host, const rsf_game_cpu_event* event) noexcept try
{
    if (host->cpu_event) host->cpu_event(host->user, event);
} catch (...) {}
