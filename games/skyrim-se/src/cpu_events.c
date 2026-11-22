/* SPDX-License-Identifier: MIT */
#include <rescaleframe/game_api.h>
#include "cpu_call.h"
#include <string.h>
#include <windows.h>

#if defined(_MSC_VER)
#include <intrin.h>
#define RSF_SKYRIM_CALLER ((uintptr_t)_ReturnAddress())
/* MSVC unwinds through the detour when the engine function raises. Leave must still run. */
#define RSF_SCOPE_TRY __try {
#define RSF_SCOPE_FINALLY(scope) } __finally { scope_leave(scope); }
#else
#define RSF_SKYRIM_CALLER ((uintptr_t)__builtin_return_address(0))
#define RSF_SCOPE_TRY {
#define RSF_SCOPE_FINALLY(scope) } scope_leave(scope);
#endif

void* rsf_skyrim_cpu_original(uint32_t);
bool rsf_skyrim_cpu_tick_admissible(void);
void rsf_skyrim_cpu_enter(void);
void rsf_skyrim_cpu_leave(void);
void rsf_skyrim_cpu_before(rsf_skyrim_cpu_hook, struct rsf_skyrim_cpu_call*, rsf_game_render_pass*);
void rsf_skyrim_cpu_after(rsf_skyrim_cpu_hook, struct rsf_skyrim_cpu_call*, rsf_game_render_pass*,
    uintptr_t, bool returned);

_Static_assert(sizeof(struct rsf_skyrim_cpu_call) == 344 && _Alignof(struct rsf_skyrim_cpu_call) == 8,
    "Skyrim CPU call layout");
_Static_assert(offsetof(struct rsf_skyrim_cpu_call, storage) == 24, "Skyrim CPU call storage offset");
_Static_assert(RSF_SKYRIM_CPU_HOOK_COUNT == 13, "Skyrim CPU hook count");
_Static_assert(sizeof(struct rsf_skyrim_menu_observation) == 3, "Skyrim menu observation layout");

typedef struct scope {
    rsf_skyrim_cpu_hook index;
    struct rsf_skyrim_cpu_call call;
    rsf_game_render_pass* pass;
    uintptr_t result;
    bool returned;
} scope;

static void scope_enter(scope* s, rsf_skyrim_cpu_hook index, uintptr_t owner, uintptr_t argument,
    uintptr_t caller, rsf_game_render_pass* pass)
{
    s->index = index;
    s->call.owner = owner;
    s->call.argument = argument;
    s->call.caller = caller;
    memset(s->call.storage, 0, sizeof(s->call.storage));
    s->pass = pass;
    s->result = 0;
    s->returned = false;
    rsf_skyrim_cpu_enter();
    rsf_skyrim_cpu_before(index, &s->call, pass);
}

static void scope_leave(scope* s)
{
    rsf_skyrim_cpu_after(s->index, &s->call, s->pass, s->result, s->returned);
    rsf_skyrim_cpu_leave();
}

static void scope_complete(scope* s, uintptr_t result)
{
    s->result = result;
    s->returned = true;
}

typedef short (*tick_fn)(void*);
typedef uintptr_t (*poll_fn)(void*, float);
typedef uintptr_t (*render_fn)(void*);
typedef int (*dispatch_fn)(void*, void*);
typedef uintptr_t (*world_fn)(void*, uint32_t, uint8_t);
typedef uintptr_t (*controls_fn)(void*, void*, void*);
typedef uintptr_t (*camera_fn)(void*);
typedef uint32_t (*jobs_fn)(void);
typedef uintptr_t (*named_poll_fn)(void*);
typedef void (*movement_fn)(void*, float, void*, float*);
typedef uintptr_t (*player_update_fn)(void*, float);
typedef void (*player_camera_fn)(void*);

short rsf_skyrim_cpu_tick(void* main)
{
    const tick_fn original = (tick_fn)rsf_skyrim_cpu_original(RSF_SKYRIM_CPU_TICK);
    scope s;
    short result;
    if (!rsf_skyrim_cpu_tick_admissible()) return original(main);
    scope_enter(&s, RSF_SKYRIM_CPU_TICK, (uintptr_t)main, 0, 0, NULL);
    RSF_SCOPE_TRY
        result = original(main);
        scope_complete(&s, (uintptr_t)result);
    RSF_SCOPE_FINALLY(&s)
    return result;
}

/* The native timestep is in XMM1. */
uintptr_t rsf_skyrim_cpu_poll(void* manager, float timestep)
{
    const poll_fn original = (poll_fn)rsf_skyrim_cpu_original(RSF_SKYRIM_CPU_POLL);
    scope s;
    uintptr_t result;
    scope_enter(&s, RSF_SKYRIM_CPU_POLL, (uintptr_t)manager, 0, 0, NULL);
    RSF_SCOPE_TRY
        result = original(manager, timestep);
        scope_complete(&s, result);
    RSF_SCOPE_FINALLY(&s)
    return result;
}

uintptr_t rsf_skyrim_cpu_render(void* main)
{
    rsf_game_render_pass pass = {0};
    scope s;
    uintptr_t result;
    scope_enter(&s, RSF_SKYRIM_CPU_RENDER, (uintptr_t)main, 0, 0, &pass);
    RSF_SCOPE_TRY
        result = ((render_fn)rsf_skyrim_cpu_original(RSF_SKYRIM_CPU_RENDER))(main);
        scope_complete(&s, result);
    RSF_SCOPE_FINALLY(&s)
    return result;
}

int rsf_skyrim_cpu_dispatch(void* manager, void* list)
{
    scope s;
    int result;
    scope_enter(&s, RSF_SKYRIM_CPU_DISPATCH, (uintptr_t)manager, (uintptr_t)list, 0, NULL);
    RSF_SCOPE_TRY
        result = ((dispatch_fn)rsf_skyrim_cpu_original(RSF_SKYRIM_CPU_DISPATCH))(manager, list);
        scope_complete(&s, (uintptr_t)result);
    RSF_SCOPE_FINALLY(&s)
    return result;
}

uintptr_t rsf_skyrim_cpu_world(void* main, uint32_t output, uint8_t flags)
{
    rsf_game_render_pass pass = {0};
    scope s;
    uintptr_t result;
    scope_enter(&s, RSF_SKYRIM_CPU_WORLD, (uintptr_t)main, output, RSF_SKYRIM_CALLER, &pass);
    RSF_SCOPE_TRY
        result = ((world_fn)rsf_skyrim_cpu_original(RSF_SKYRIM_CPU_WORLD))(main, output, flags);
        scope_complete(&s, result);
    RSF_SCOPE_FINALLY(&s)
    return result;
}

uintptr_t rsf_skyrim_cpu_controls(void* owner, void* list, void* source)
{
    scope s;
    uintptr_t result;
    scope_enter(&s, RSF_SKYRIM_CPU_CONTROLS, (uintptr_t)owner, (uintptr_t)list, 0, NULL);
    RSF_SCOPE_TRY
        /* The event-source pointer in R8 is forwarded unchanged. */
        result = ((controls_fn)rsf_skyrim_cpu_original(RSF_SKYRIM_CPU_CONTROLS))(owner, list, source);
        scope_complete(&s, result);
    RSF_SCOPE_FINALLY(&s)
    return result;
}

uintptr_t rsf_skyrim_cpu_camera_update(void* state)
{
    scope s;
    uintptr_t result;
    scope_enter(&s, RSF_SKYRIM_CPU_CAMERA, (uintptr_t)state, 0, 0, NULL);
    RSF_SCOPE_TRY
        result = ((camera_fn)rsf_skyrim_cpu_original(RSF_SKYRIM_CPU_CAMERA))(state);
        scope_complete(&s, result);
    RSF_SCOPE_FINALLY(&s)
    return result;
}

uint32_t rsf_skyrim_cpu_jobs_submit(void)
{
    scope s;
    uint32_t result;
    scope_enter(&s, RSF_SKYRIM_CPU_SUBMIT, 0, 0, RSF_SKYRIM_CALLER, NULL);
    RSF_SCOPE_TRY
        result = ((jobs_fn)rsf_skyrim_cpu_original(RSF_SKYRIM_CPU_SUBMIT))();
        scope_complete(&s, result);
    RSF_SCOPE_FINALLY(&s)
    return result;
}

uint32_t rsf_skyrim_cpu_jobs_join(void)
{
    scope s;
    uint32_t result;
    scope_enter(&s, RSF_SKYRIM_CPU_JOIN, 0, 0, RSF_SKYRIM_CALLER, NULL);
    RSF_SCOPE_TRY
        result = ((jobs_fn)rsf_skyrim_cpu_original(RSF_SKYRIM_CPU_JOIN))();
        scope_complete(&s, result);
    RSF_SCOPE_FINALLY(&s)
    return result;
}

uintptr_t rsf_skyrim_cpu_named_poll(void* data)
{
    scope s;
    uintptr_t result;
    scope_enter(&s, RSF_SKYRIM_CPU_NAMED_POLL, 0, 0, RSF_SKYRIM_CALLER, NULL);
    RSF_SCOPE_TRY
        result = ((named_poll_fn)rsf_skyrim_cpu_original(RSF_SKYRIM_CPU_NAMED_POLL))(data);
        scope_complete(&s, result);
    RSF_SCOPE_FINALLY(&s)
    return result;
}

void rsf_skyrim_cpu_movement(void* actor, float timestep, void* argument, float* output)
{
    scope s;
    scope_enter(&s, RSF_SKYRIM_CPU_MOVEMENT, (uintptr_t)actor, 0, 0, NULL);
    RSF_SCOPE_TRY
        ((movement_fn)rsf_skyrim_cpu_original(RSF_SKYRIM_CPU_MOVEMENT))(actor, timestep, argument, output);
        scope_complete(&s, 0);
    RSF_SCOPE_FINALLY(&s)
}

uintptr_t rsf_skyrim_cpu_player_update(void* actor, float timestep)
{
    scope s;
    uintptr_t result;
    scope_enter(&s, RSF_SKYRIM_CPU_PLAYER_UPDATE, (uintptr_t)actor, 0, 0, NULL);
    RSF_SCOPE_TRY
        result = ((player_update_fn)rsf_skyrim_cpu_original(RSF_SKYRIM_CPU_PLAYER_UPDATE))(actor, timestep);
        scope_complete(&s, result);
    RSF_SCOPE_FINALLY(&s)
    return result;
}

/* TESCamera::Update forwards to the current state vfunc and has a void result. */
void rsf_skyrim_cpu_player_camera_update(void* camera)
{
    scope s;
    scope_enter(&s, RSF_SKYRIM_CPU_PLAYER_CAMERA, (uintptr_t)camera, 0, 0, NULL);
    RSF_SCOPE_TRY
        ((player_camera_fn)rsf_skyrim_cpu_original(RSF_SKYRIM_CPU_PLAYER_CAMERA))(camera);
        scope_complete(&s, 0);
    RSF_SCOPE_FINALLY(&s)
}

/* The engine's UI object is foreign C++. SEH also contains its C++ exceptions. */
static void read_menus(uintptr_t open_address, uintptr_t frozen_address, uintptr_t ui, uintptr_t loading,
    uintptr_t main, struct rsf_skyrim_menu_observation* facts)
{
    bool (*open)(void*, const void*) = (bool (*)(void*, const void*))open_address;
    bool (*frozen)(void*) = (bool (*)(void*))frozen_address;
    void* owner = (void*)ui;
    facts->loading_menu = open(owner, (const void*)loading);
    facts->main_menu = open(owner, (const void*)main);
    facts->frozen_background = frozen(owner);
}

bool rsf_skyrim_query_menus(uintptr_t open_address, uintptr_t frozen_address, uintptr_t ui,
    uintptr_t loading, uintptr_t main, struct rsf_skyrim_menu_observation* facts)
{
#if defined(_MSC_VER)
    __try {
        read_menus(open_address, frozen_address, ui, loading, main, facts);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
#else
    read_menus(open_address, frozen_address, ui, loading, main, facts);
    return true;
#endif
}

void rsf_skyrim_cpu_event_guarded(const rsf_game_host_services* host, const rsf_game_cpu_event* event)
{
#if defined(_MSC_VER)
    __try {
        host->cpu_event(host->user, event);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
    }
#else
    host->cpu_event(host->user, event);
#endif
}