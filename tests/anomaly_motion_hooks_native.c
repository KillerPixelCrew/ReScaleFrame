/* SPDX-License-Identifier: MIT */
/* Native launcher for the Rust fixture (motion/hook_fixture.rs). The real CBackend::constants.flush detour
   targets this executable's local function, with the original hook ABI. Rust names the flush site's index in the
   geometry profile (geometry_profile.rs). */
#include "native_geometry.h"
#include "native_hooks.h"
#include <windows.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#ifdef _MSC_VER
#define RSF_NOINLINE __declspec(noinline)
#else
#define RSF_NOINLINE __attribute__((noinline))
#endif

typedef struct facts {
    uint8_t in_visual, view, domain, capture, kind, verdict, visual_type;
} facts;
typedef void (*sink_fn)(const facts*);
_Static_assert(sizeof(facts) == 7, "facts record");

static sink_fn sink;
static unsigned original_calls;
static rsf_anomaly_hook_set hooks;

RSF_NOINLINE static void flush_fixture(void)
{
    sink(NULL);
    if (++original_calls > 1000) abort();
}

static void callback(const rsf_anomaly_geometry_draw_facts* draw)
{
    const facts record = {(uint8_t)draw->in_visual, draw->view, draw->domain, (uint8_t)(draw->capture != NULL),
        draw->kind, draw->verdict, draw->type};
    sink(&record);
}

static bool install(sink_fn emit, size_t flush_site)
{
    sink = emit;
    uint8_t* base = (uint8_t*)GetModuleHandleW(NULL);
    const IMAGE_DOS_HEADER* dos = (const IMAGE_DOS_HEADER*)base;
    const IMAGE_NT_HEADERS64* pe = (const IMAGE_NT_HEADERS64*)(base + dos->e_lfanew);
    const rsf_anomaly_hook_site* sites = rsf_anomaly_geometry_hook_sites(0);
    if (!sites) return false;
    rsf_anomaly_hook_site redirect = sites[flush_site];
    const uint8_t* local = (const uint8_t*)(uintptr_t)&flush_fixture;
    redirect.rva = (uint32_t)(local - base);
    memcpy(redirect.expected, local, sizeof(redirect.expected));
    return rsf_anomaly_hooks_install(&hooks, base, pe->OptionalHeader.SizeOfImage, &redirect, 1);
}

static void observe(bool enabled) { rsf_anomaly_geometry_set_flush(enabled ? callback : NULL); }
static void flush(void) { flush_fixture(); }
static bool disable(void) { return rsf_anomaly_hooks_disable(&hooks); }

// A context scope, then a first-person context and an unidentified skinned visual inside it. Scopes leave in
// reverse order.
static bool flush_scoped(void)
{
    rsf_anomaly_geometry_context context;
    memset(&context, 0, sizeof(context));
    rsf_anomaly_geometry_context_enter(&context, RSF_ANOMALY_GEOMETRY_SCOPE, RSF_ANOMALY_GEOMETRY_WORLD, false);
    flush_fixture();
    rsf_anomaly_geometry_context hud;
    memset(&hud, 0, sizeof(hud));
    rsf_anomaly_geometry_context_enter(&hud, RSF_ANOMALY_GEOMETRY_UNSET, RSF_ANOMALY_GEOMETRY_FIRST_PERSON, true);
    rsf_anomaly_geometry_registry* registry = rsf_anomaly_geometry_registry_create();
    rsf_anomaly_geometry_visual node;
    memset(&node, 0, sizeof(node));
    node.registry = registry;
    node.kind = RSF_ANOMALY_GEOMETRY_HARDWARE_SKINNED;
    node.pass = RSF_ANOMALY_PASS_MAIN;
    node.type = RSF_ANOMALY_TYPE_SKELETON_ST;
    node.verdict = RSF_ANOMALY_DRAW_UNIDENTIFIED_DYNAMIC;
    rsf_anomaly_geometry_visual_enter(&node);
    flush_fixture();
    rsf_anomaly_geometry_visual_leave(&node);
    rsf_anomaly_geometry_registry_destroy(registry);
    rsf_anomaly_geometry_context_leave(&hud);
    rsf_anomaly_geometry_context_leave(&context);
    return true;
}

typedef struct fixture {
    bool (*install)(sink_fn, size_t flush_site);
    void (*observe)(bool);
    void (*flush)(void);
    bool (*flush_scoped)(void);
    bool (*disable)(void);
} fixture;

int rsf_anomaly_motion_hooks_fixture(const fixture*);

int main(void)
{
    static const fixture table = {install, observe, flush, flush_scoped, disable};
    const int status = rsf_anomaly_motion_hooks_fixture(&table);
    observe(false);
    return status;
}
