/* SPDX-License-Identifier: MIT */
/* The geometry detours against real MinHook trampolines on fixture functions, plus the production guarded install
 * against a synthetic image. No game code runs. Rust drives the scenarios in their original order. */
#include "native_geometry.h"
#include "native_geometry/fixture_platform.h"
#include "native_memory.h"
#include <windows.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(x) do { if (!(x)) { fprintf(stderr, "%d: %s\n", __LINE__, #x); exit(1); } } while (0)

enum { UNSET = RSF_ANOMALY_GEOMETRY_UNSET };
enum { MAIN = RSF_ANOMALY_GEOMETRY_MAIN, SCOPE = RSF_ANOMALY_GEOMETRY_SCOPE };
enum { WORLD = RSF_ANOMALY_GEOMETRY_WORLD, FIRST_PERSON = RSF_ANOMALY_GEOMETRY_FIRST_PERSON };
enum { PHASE_NORMAL = 0, PHASE_SMAP = 1 };
/* geometry_profile.rs layout shared by both builds. */
enum {
    SECOND_VIEWPORT_DELAY_OFFSET = 1, CONSTANT_BUFFER_SLOTS = 14, CONSTANT_BUFFER_SIZE_OFFSET = 0x58,
    CONSTANT_BUFFER_DATA_OFFSET = 0x60, TREE_PM_LAST_LOD_OFFSET = 0x108, HIERARCHY_CHILDREN_OFFSET = 96,
};
#define SITE_COUNT 17u

bool rsf_test_anomaly_geometry_redirects(const uint8_t*, uint32_t, rsf_anomaly_geometry_read_fn,
    const rsf_anomaly_hook_site*, size_t, void* (*)[15], rsf_anomaly_hook_site*);
size_t rsf_test_anomaly_select_build(int, const char* const*);
void rsf_test_anomaly_announce_build(const char*, const char*);

typedef void* (*create_fn)(void*, const char*, void*);
typedef void* (*duplicate_fn)(void*, void*);
typedef void (*delete_fn)(void*, void**, int32_t);
typedef void (*render_fn)(void*, float);
typedef void (*flush_fn)(void);
typedef void (*unary_fn)(void*);

static unsigned char visual[512], child[512], duplicate_visual[512], unregistered[512], tree_visual[512];
static rsf_anomaly_geometry_key retirements[64];
static size_t retirement_count;
static void* expected_owner;
static const char* const expected_name = "geometry-fixture.ogf";
static void* expected_reader = child;
static bool retiring;
static uint64_t observed_generation;
static bool constructing;
/* What the next render fixture invocation expects. The fixture stands in for the engine's Fvisual::Render, which
 * reaches the D3D draw through CBackend::constants.flush. */
static void* expected_instance = child;
static bool expect_identity = true;
static bool call_flush;
static bool valid_pm_palette = true;
static uint8_t expected_view = SCOPE, expected_domain = FIRST_PERSON;
static flush_fn flush_target;
static unary_fn near_target, normal_target, emissive_target;
static unsigned char* phase_at_flush;
static bool has_flush_phase;
static uint32_t flush_phase;
static unsigned char* world_at_flush;
static uint32_t flush_calls;
static uint32_t detail_calls;

static const rsf_anomaly_geometry_visual* borrowed(void)
{
    return (const rsf_anomaly_geometry_visual*)rsf_anomaly_geometry_borrow_draw();
}

static __declspec(noinline) void* duplicate_fixture(void*, void*);
static __declspec(noinline) void render_fixture(void*, float);

static __declspec(noinline) void* create_fixture(void* owner, const char* name, void* reader)
{
    CHECK(owner == expected_owner && name == expected_name && reader == expected_reader);
    constructing = true;
    CHECK(duplicate_fixture(owner, visual) == child);
    render_fixture(child, 0.375f); /* No identity before the enclosing model is returned. */
    constructing = false;
    return visual;
}

static __declspec(noinline) void* duplicate_fixture(void* owner, void* source)
{
    CHECK(owner == expected_owner && source == visual);
    return constructing ? child : duplicate_visual;
}

static __declspec(noinline) void delete_fixture(void* owner, void** pointer, int32_t discard)
{
    CHECK(owner == expected_owner && *pointer == visual && discard == 0x12345678);
    CHECK(retirement_count >= 2 && !retiring);
    *pointer = NULL;
}

/* A copy of the live capture of this thread. */
typedef struct captured {
    bool present, has_world;
    double world_x;
    size_t bone_count;
    uint64_t palette_layout;
    double first_bone_x, last_bone_x;
} captured;

static captured capture_now(void)
{
    rsf_anomaly_geometry_capture_storage storage;
    captured out;
    memset(&out, 0, sizeof(out));
    const rsf_anomaly_geometry_snapshot* snapshot = rsf_anomaly_geometry_capture_current(&storage);
    if (!snapshot) return out;
    out.present = true;
    out.has_world = snapshot->world != NULL;
    if (snapshot->world) out.world_x = snapshot->world->values[3];
    out.bone_count = snapshot->bone_count;
    out.palette_layout = snapshot->palette_layout;
    if (snapshot->bone_count) {
        out.first_bone_x = snapshot->bones[0].values[3];
        out.last_bone_x = snapshot->bones[snapshot->bone_count - 1].values[3];
    }
    rsf_anomaly_geometry_capture_release(&storage);
    return out;
}

static __declspec(noinline) void render_fixture(void* instance, float lod)
{
    CHECK(instance == expected_instance && lod == 0.375f);
    const rsf_anomaly_geometry_visual* identity = borrowed();
    if (constructing) { CHECK(!identity); return; }
    if (!expect_identity) {
        CHECK(!identity);
    } else {
        CHECK(identity && identity->key.owner == (uintptr_t)expected_owner);
        CHECK(identity->key.visual == (uintptr_t)child);
        CHECK(identity->domain == expected_domain && identity->view == expected_view);
        const captured capture = capture_now();
        CHECK(capture.present && capture.has_world && capture.world_x == 123);
        observed_generation = identity->key.generation;
    }
    if (call_flush) {
        uint32_t entry_phase = 0;
        if (has_flush_phase) {
            memcpy(&entry_phase, phase_at_flush, sizeof(entry_phase));
            memcpy(phase_at_flush, &flush_phase, sizeof(flush_phase));
        }
        flush_target();
        if (has_flush_phase) memcpy(phase_at_flush, &entry_phase, sizeof(entry_phase));
    }
}

static __declspec(noinline) void skin_st_fixture(void* instance, float lod)
{
    CHECK(instance == child && lod == 0.625f);
    CHECK(borrowed() && borrowed()->kind == RSF_ANOMALY_GEOMETRY_HARDWARE_SKINNED);
    const captured capture = capture_now();
    CHECK(capture.present && capture.bone_count == 1 && capture.first_bone_x == 456);
}

static __declspec(noinline) void skin_pm_fixture(void* instance, float lod)
{
    CHECK(instance == child && lod == 0.875f);
    CHECK(borrowed() && borrowed()->kind == RSF_ANOMALY_GEOMETRY_HARDWARE_SKINNED);
    const captured capture = capture_now();
    CHECK(capture.present && capture.has_world);
    if (valid_pm_palette) {
        CHECK(capture.palette_layout != 0 && capture.bone_count == 65);
        CHECK(capture.first_bone_x == 789 && capture.last_bone_x == 853);
    } else {
        CHECK(capture.palette_layout == 0 && capture.bone_count == 0);
    }
}

static __declspec(noinline) void flush_fixture(void)
{
    ++flush_calls;
    if (world_at_flush) {
        const float translation = 321;
        memcpy(world_at_flush + 12 * sizeof(float), &translation, sizeof(translation));
    }
    if (flush_calls == 0xffffffffu) abort();
}

/* Foliage fixtures: a tree visual and CDetailManager::Render, both reaching the D3D draw through the flush. */
static __declspec(noinline) void tree_st_fixture(void* instance, float lod)
{
    CHECK(instance == tree_visual && lod == 0.5f);
    flush_target();
}

static __declspec(noinline) void tree_pm_fixture(void* instance, float lod)
{
    CHECK(instance == tree_visual && lod == 0.25f);
    flush_target();
}

static __declspec(noinline) void detail_fixture(void* self)
{
    CHECK(self == &detail_calls);
    ++detail_calls;
    flush_target();
    flush_target();
}

/* What the flush callback saw. The capture pointer is not kept. */
#define MAX_FACTS 8
static rsf_anomaly_geometry_draw_facts seen_facts[MAX_FACTS];
static size_t seen_count;
static bool callback_has_translation;
static double callback_translation;
static uint32_t callback_flush_calls;

static void collect_facts(const rsf_anomaly_geometry_draw_facts* facts)
{
    callback_flush_calls = flush_calls;
    callback_has_translation = facts->capture && facts->capture->world;
    callback_translation = callback_has_translation ? facts->capture->world->values[3] : 0;
    CHECK(seen_count < MAX_FACTS);
    seen_facts[seen_count] = *facts;
    seen_facts[seen_count++].capture = NULL;
}

typedef struct context {
    uint8_t view, domain;
} context;

static bool same_context(context a, context b) { return a.view == b.view && a.domain == b.domain; }

/* Observes the draw context a visual drawn right now would receive. */
static context context_now(void)
{
    static rsf_anomaly_geometry_registry* registry;
    static rsf_anomaly_geometry_key key;
    if (!registry) {
        registry = rsf_anomaly_geometry_registry_create();
        key = (rsf_anomaly_geometry_key){1, 2, rsf_anomaly_geometry_register(registry, 1, 2)};
    }
    rsf_anomaly_geometry_visual probe;
    memset(&probe, 0, sizeof(probe));
    probe.registry = registry;
    probe.key = key;
    probe.kind = RSF_ANOMALY_GEOMETRY_RIGID;
    rsf_anomaly_geometry_visual_enter(&probe);
    const rsf_anomaly_geometry_visual* identity = borrowed();
    CHECK(identity);
    const context result = {identity->view, identity->domain};
    rsf_anomaly_geometry_visual_leave(&probe);
    return result;
}

static context hud_context, sorted_before, sorted_hud, sorted_after, emissive_before, emissive_hud, emissive_after;
static bool nested_window;
static uint32_t hud_calls, near_calls, normal_calls;

static __declspec(noinline) void hud_fixture(void* self)
{
    CHECK(self == &hud_calls);
    hud_context = context_now();
    ++hud_calls;
}

static __declspec(noinline) void near_fixture(void* self)
{
    CHECK(self == &hud_calls);
    ++near_calls;
    if (!near_calls) abort();
}

static __declspec(noinline) void normal_fixture(void* self)
{
    CHECK(self == &hud_calls);
    ++normal_calls;
    if (!normal_calls) abort();
}

static __declspec(noinline) void sorted_fixture(void* self)
{
    CHECK(self == &hud_calls);
    sorted_before = context_now();
    near_target(self);
    sorted_hud = context_now();
    if (nested_window) {
        {
            rsf_anomaly_geometry_context child_view;
            rsf_anomaly_geometry_context_enter(&child_view, MAIN, WORLD, false);
            emissive_target(self);
            CHECK(same_context(context_now(), (context){MAIN, WORLD}));
            rsf_anomaly_geometry_context_leave(&child_view);
        }
        CHECK(same_context(context_now(), sorted_hud));
    }
    normal_target(self);
    sorted_after = context_now();
}

static __declspec(noinline) void emissive_fixture(void* self)
{
    CHECK(self == &hud_calls);
    emissive_before = context_now();
    near_target(self);
    emissive_hud = context_now();
    if (nested_window) {
        normal_target(self);
        emissive_after = context_now();
    }
    /* A function that leaves without rmNormal must still close the window before the enclosing scopes unwind. */
}

static void retired(void* user, const rsf_anomaly_geometry_key* key)
{
    (void)user;
    retiring = true;
    CHECK(retirement_count < 64);
    retirements[retirement_count++] = *key;
    retiring = false;
}

/* A draw sink copy: the capture is only valid through the synchronous callback. */
typedef struct sunk_copy {
    rsf_anomaly_geometry_key key;
    uint8_t view, domain;
    bool has_world;
    double world_x;
} sunk_copy;
static sunk_copy sunk[8];
static size_t sunk_count;

static void collect_draw(void* user, const rsf_anomaly_geometry_snapshot* capture)
{
    (void)user;
    CHECK(sunk_count < 8);
    sunk_copy* copy = &sunk[sunk_count++];
    copy->key = capture->key;
    copy->view = capture->view;
    copy->domain = capture->domain;
    copy->has_world = capture->world != NULL;
    copy->world_x = capture->world ? capture->world->values[3] : 0;
}

#define MAX_LINES 64
static char lines[MAX_LINES][640];
static size_t line_count;

static void collect_line(void* user, const char* line)
{
    (void)user;
    CHECK(line_count < MAX_LINES);
    strncpy_s(lines[line_count++], sizeof(lines[0]), line, _TRUNCATE);
}

/* How much the draw count of one (pass, type, verdict) cell grew between two coverage reads. */
static uint64_t cell_delta(const rsf_anomaly_geometry_coverage* after, const rsf_anomaly_geometry_coverage* before,
    unsigned pass, unsigned type, unsigned verdict)
{
    return after->draws[pass][type][verdict] - before->draws[pass][type][verdict];
}

static rsf_anomaly_geometry_coverage coverage_now(void)
{
    rsf_anomaly_geometry_coverage out;
    rsf_anomaly_geometry_coverage_read(&out);
    return out;
}

static rsf_anomaly_geometry_stats stats_now(void)
{
    rsf_anomaly_geometry_stats out;
    rsf_anomaly_geometry_stats_read(&out);
    return out;
}

static bool read_leaf(void* out, const void* address, size_t size) { return guarded_copy(out, address, size); }

static unsigned char* allocate_fixture_image(uint32_t size)
{
    /* Production installation path, using guarded code copied into an executable-memory fixture.
     * No game code is executed, no game process or installation is modified. */
    unsigned char* image = VirtualAlloc(NULL, size, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE);
    CHECK(image);
    IMAGE_DOS_HEADER* fixture_dos = (IMAGE_DOS_HEADER*)image;
    fixture_dos->e_magic = IMAGE_DOS_SIGNATURE;
    fixture_dos->e_lfanew = 0x80;
    IMAGE_NT_HEADERS64* fixture_pe = (IMAGE_NT_HEADERS64*)(image + fixture_dos->e_lfanew);
    fixture_pe->Signature = IMAGE_NT_SIGNATURE;
    fixture_pe->FileHeader.Machine = IMAGE_FILE_MACHINE_AMD64;
    fixture_pe->OptionalHeader.Magic = IMAGE_NT_OPTIONAL_HDR64_MAGIC;
    fixture_pe->OptionalHeader.SizeOfImage = size;
    return image;
}

/* Shared state of the scenarios. */
static size_t build, other;
static rsf_test_anomaly_geometry_profile gp;
static unsigned char* module;
static rsf_anomaly_hook_set hooks;
static void* fixtures[15];
static create_fn create;
static duplicate_fn duplicate;
static delete_fn destroy;
static render_fn render, tree_st, tree_pm;
static unary_fn hud, sorted, emissive, detail_render;
static unsigned char parent[256];
static unsigned char bone[65 * 168];
static void* child_list[1];
static int owner;
static const float world[16] = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 123, 0, 0, 1};
static const float identity_world[16] = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};

static void set_u32(uint32_t rva, uint32_t value) { memcpy(module + rva, &value, sizeof(value)); }

static void check_skin(size_t index, size_t offset, float lod, float translation)
{
    unsigned char* parent_pointer = parent;
    const uint16_t mode = 2;
    const uint32_t count = index == 4 ? 1 : 65;
    memcpy(child + offset + 8, &parent_pointer, sizeof(parent_pointer));
    memcpy(child + offset + 56, &mode, sizeof(mode));
    memcpy(child + offset + 60, &count, sizeof(count));
    float transform[16];
    memcpy(transform, world, sizeof(transform));
    for (uint32_t i = 0; i < count; ++i) {
        transform[12] = translation + (float)i;
        memcpy(bone + i * 168 + 64, transform, sizeof(transform));
    }
    ((render_fn)fixtures[index])(child, lod);
}

static void run_lifecycle(const void* unused)
{
    (void)unused;
    /* Prime the typed fixture trampolines. */
    CHECK(create(&owner, expected_name, expected_reader) == visual);
    CHECK(duplicate(&owner, visual) == duplicate_visual);
    rsf_anomaly_geometry_context scope;
    rsf_anomaly_geometry_context_enter(&scope, SCOPE, FIRST_PERSON, false);
    render(child, 0.375f);
    rsf_anomaly_geometry_context_leave(&scope);
    const uint64_t first_generation = observed_generation;
    unsigned char* bone_pointer = bone;
    memcpy(parent + 208, &bone_pointer, sizeof(bone_pointer));
    check_skin(4, 160, 0.625f, 456);
    check_skin(5, 208, 0.875f, 789);
    /* A bad affine lane in the second batch discards all 64 earlier matrices. */
    const float bad_lane = NAN;
    memcpy(bone + 64 * 168 + 64, &bad_lane, sizeof(bad_lane));
    valid_pm_palette = false;
    ((render_fn)fixtures[5])(child, 0.875f);
    void* pointer = visual;
    destroy(&owner, &pointer, 0x12345678);
    CHECK(!pointer && !borrowed());
    CHECK(create(&owner, expected_name, expected_reader) == visual);
    rsf_anomaly_geometry_context_enter(&scope, SCOPE, FIRST_PERSON, false);
    render(child, 0.375f);
    rsf_anomaly_geometry_context_leave(&scope);
    CHECK(observed_generation > first_generation);
}

static void run_flush_capture(const void* unused)
{
    (void)unused;
    /* Draw-time call site: the hooked CBackend::constants.flush reaches the capture for an identified main-pass draw
     * and hands the copy to the sink. The registered visual stays child. */
    rsf_anomaly_geometry_set_draw(0, collect_draw);
    call_flush = true;
    rsf_anomaly_geometry_context scope;
    {
        const rsf_anomaly_geometry_coverage before = coverage_now();
        const uint32_t calls_before = flush_calls;
        rsf_anomaly_geometry_context_enter(&scope, SCOPE, FIRST_PERSON, false);
        render(child, 0.375f);
        rsf_anomaly_geometry_context_leave(&scope);
        const rsf_anomaly_geometry_coverage after = coverage_now();
        CHECK(flush_calls == calls_before + 1); /* The original flush still runs. */
        CHECK(sunk_count == 1 && sunk[0].key.visual == (uintptr_t)child);
        CHECK(sunk[0].view == SCOPE && sunk[0].domain == FIRST_PERSON);
        CHECK(sunk[0].has_world && sunk[0].world_x == 123);
        CHECK(cell_delta(&after, &before, RSF_ANOMALY_PASS_MAIN, RSF_ANOMALY_TYPE_STATIC, RSF_ANOMALY_DRAW_IDENTIFIED) == 1);
        CHECK(after.flush_total[0] == before.flush_total[0] + 1 &&
            after.flush_in_visual[0] == before.flush_in_visual[0] + 1);
        CHECK(after.captured == before.captured + 1 && after.sunk == before.sunk + 1 &&
            after.capture_no_world == before.capture_no_world);
    }
    /* A flush outside every visual is counted as such and captures nothing. */
    {
        const rsf_anomaly_geometry_coverage before = coverage_now();
        flush_target();
        const rsf_anomaly_geometry_coverage after = coverage_now();
        CHECK(after.flush_total[0] == before.flush_total[0] + 1 &&
            after.flush_in_visual[0] == before.flush_in_visual[0]);
        CHECK(after.captured == before.captured && sunk_count == 1);
    }
    /* The shared callback observes the same copy after the original flush, even
     * if the engine changes its CPU world matrix during that call. */
    {
        const uint32_t calls_before = flush_calls;
        rsf_anomaly_geometry_set_flush(collect_facts);
        world_at_flush = module + gp.world;
        rsf_anomaly_geometry_context_enter(&scope, SCOPE, FIRST_PERSON, false);
        render(child, 0.375f);
        CHECK(callback_flush_calls == calls_before + 1 && callback_has_translation && callback_translation == 123);
        float sampled[16] = {0};
        CHECK(rsf_anomaly_geometry_world_matrix(sampled) && sampled[12] == 321);
        CHECK(sunk_count == 2 && sunk[1].has_world && sunk[1].world_x == 123);
        rsf_anomaly_geometry_context_leave(&scope);
        memcpy(world_at_flush, world, sizeof(world));
        world_at_flush = NULL;
        rsf_anomaly_geometry_set_flush(NULL);
        seen_count = 0;
    }
    /* An identified draw with no view/projection context is still captured; the missing context is counted. */
    {
        expected_view = UNSET;
        expected_domain = UNSET;
        const rsf_anomaly_geometry_coverage before = coverage_now();
        sunk_count = 0;
        render(child, 0.375f);
        const rsf_anomaly_geometry_coverage after = coverage_now();
        CHECK(sunk_count == 1 && sunk[0].view == UNSET && sunk[0].domain == UNSET);
        CHECK(after.capture_no_context == before.capture_no_context + 1);
        expected_view = SCOPE;
        expected_domain = FIRST_PERSON;
    }
}

static void run_phase(const void* unused)
{
    (void)unused;
    rsf_anomaly_geometry_context scope;
    /* Shadow-map and Z-fill phase: nothing is captured, and the counters say why. */
    {
        set_u32(gp.phase, PHASE_SMAP);
        const rsf_anomaly_geometry_coverage before = coverage_now();
        sunk_count = 0;
        rsf_anomaly_geometry_context_enter(&scope, SCOPE, FIRST_PERSON, false);
        render(child, 0.375f);
        rsf_anomaly_geometry_context_leave(&scope);
        const rsf_anomaly_geometry_coverage after = coverage_now();
        CHECK(sunk_count == 0 && after.captured == before.captured);
        CHECK(cell_delta(&after, &before, RSF_ANOMALY_PASS_SHADOW, RSF_ANOMALY_TYPE_STATIC, RSF_ANOMALY_DRAW_IGNORED_SHADOW) == 1);
        CHECK(cell_delta(&after, &before, RSF_ANOMALY_PASS_MAIN, RSF_ANOMALY_TYPE_STATIC, RSF_ANOMALY_DRAW_IDENTIFIED) == 0);
        CHECK(after.flush_total[1] == before.flush_total[1] + 1 &&
            after.flush_in_visual[1] == before.flush_in_visual[1] + 1);
        set_u32(gp.phase, 7);
        rsf_anomaly_geometry_context_enter(&scope, SCOPE, FIRST_PERSON, false);
        render(child, 0.375f);
        rsf_anomaly_geometry_context_leave(&scope);
        const rsf_anomaly_geometry_coverage unknown = coverage_now();
        CHECK(cell_delta(&unknown, &after, RSF_ANOMALY_PASS_UNKNOWN, RSF_ANOMALY_TYPE_STATIC, RSF_ANOMALY_DRAW_IDENTIFIED) == 1);
        CHECK(sunk_count == 0 && unknown.captured == before.captured);
        set_u32(gp.phase, PHASE_NORMAL);
    }
    /* Entry classification and actual flush phase are separate facts. An
     * identified main entry cannot put a shadow or unknown flush into history. */
    {
        const rsf_anomaly_geometry_coverage before = coverage_now();
        const uint32_t calls_before = flush_calls;
        rsf_anomaly_geometry_context_enter(&scope, SCOPE, FIRST_PERSON, false);
        rsf_anomaly_geometry_set_flush(collect_facts);
        seen_count = 0;
        callback_has_translation = false;
        const uint32_t phases[2] = {PHASE_SMAP, 7};
        for (size_t i = 0; i < 2; ++i) {
            has_flush_phase = true;
            flush_phase = phases[i];
            render(child, 0.375f);
        }
        has_flush_phase = false;
        rsf_anomaly_geometry_set_flush(NULL);
        rsf_anomaly_geometry_context_leave(&scope);
        const rsf_anomaly_geometry_coverage after = coverage_now();
        CHECK(flush_calls == calls_before + 2 && sunk_count == 0 && after.captured == before.captured);
        CHECK(seen_count == 2 && !callback_has_translation);
        CHECK(seen_facts[0].pass == RSF_ANOMALY_PASS_SHADOW && seen_facts[1].pass == RSF_ANOMALY_PASS_UNKNOWN);
        for (size_t i = 0; i < seen_count; ++i)
            CHECK(seen_facts[i].in_visual && seen_facts[i].visual_pass == RSF_ANOMALY_PASS_MAIN &&
                seen_facts[i].verdict == RSF_ANOMALY_DRAW_IDENTIFIED);
    }
}

static void run_identities(const void* unused)
{
    (void)unused;
    /* Unregistered rigid visuals: level geometry (exactly Fidentity) needs no object motion; anything else is a gap. */
    expect_identity = false;
    expected_instance = unregistered;
    {
        memcpy(module + gp.world, identity_world, sizeof(identity_world));
        const rsf_anomaly_geometry_coverage before = coverage_now();
        sunk_count = 0;
        render(unregistered, 0.375f);
        const rsf_anomaly_geometry_coverage after = coverage_now();
        CHECK(cell_delta(&after, &before, RSF_ANOMALY_PASS_MAIN, RSF_ANOMALY_TYPE_STATIC, RSF_ANOMALY_DRAW_IGNORED_STATIC_WORLD) == 1);
        CHECK(sunk_count == 0 && after.captured == before.captured);
        CHECK(after.flush_in_visual[0] == before.flush_in_visual[0] + 1);

        memcpy(module + gp.world, world, sizeof(world));
        render(unregistered, 0.375f);
        const rsf_anomaly_geometry_coverage moving = coverage_now();
        CHECK(cell_delta(&moving, &after, RSF_ANOMALY_PASS_MAIN, RSF_ANOMALY_TYPE_STATIC, RSF_ANOMALY_DRAW_UNIDENTIFIED_DYNAMIC) == 1);
        CHECK(sunk_count == 0 && moving.captured == before.captured);

        set_u32(gp.phase, PHASE_SMAP);
        render(unregistered, 0.375f);
        set_u32(gp.phase, PHASE_NORMAL);
        const rsf_anomaly_geometry_coverage shadow = coverage_now();
        CHECK(cell_delta(&shadow, &moving, RSF_ANOMALY_PASS_SHADOW, RSF_ANOMALY_TYPE_STATIC, RSF_ANOMALY_DRAW_IGNORED_SHADOW) == 1);
    }
    /* The same visual live under two owners cannot be named, and says so instead of guessing. */
    {
        int other_owner = 0;
        expected_owner = &other_owner;
        expected_instance = child; /* The factory fixture draws the child once while constructing. */
        CHECK(create(&other_owner, expected_name, expected_reader) == visual);
        expected_owner = &owner;
        const rsf_anomaly_geometry_coverage before = coverage_now();
        sunk_count = 0;
        render(child, 0.375f);
        const rsf_anomaly_geometry_coverage after = coverage_now();
        CHECK(cell_delta(&after, &before, RSF_ANOMALY_PASS_MAIN, RSF_ANOMALY_TYPE_STATIC, RSF_ANOMALY_DRAW_UNIDENTIFIED_AMBIGUOUS) == 1);
        CHECK(sunk_count == 0);
        void* second = visual;
        expected_owner = &other_owner;
        destroy(&other_owner, &second, 0x12345678);
        expected_owner = &owner;
        CHECK(!second);
        expect_identity = true;
        rsf_anomaly_geometry_context scope;
        rsf_anomaly_geometry_context_enter(&scope, SCOPE, FIRST_PERSON, false);
        render(child, 0.375f);
        rsf_anomaly_geometry_context_leave(&scope);
        CHECK(sunk_count == 1);
    }
    call_flush = false;
    rsf_anomaly_geometry_set_draw(0, NULL);
}

static void run_foliage(const void* unused)
{
    (void)unused;
    /* Foliage. A level tree never enters the model registry. Without foliage identity it is the measured gap
     * (unidentified_deformed); with it the tree is named by its address, nothing is captured, and the flush facts carry
     * the visual, its type and the progressive tree's slide window. */
    expect_identity = false;
    rsf_anomaly_geometry_set_flush(collect_facts);
    const uint32_t lod5 = 5;
    memcpy(tree_visual + TREE_PM_LAST_LOD_OFFSET, &lod5, sizeof(lod5));
    CHECK(!rsf_anomaly_geometry_foliage());
    rsf_anomaly_geometry_coverage before = coverage_now();
    seen_count = 0;
    tree_st(tree_visual, 0.5f);
    rsf_anomaly_geometry_coverage after = coverage_now();
    CHECK(cell_delta(&after, &before, RSF_ANOMALY_PASS_MAIN, RSF_ANOMALY_TYPE_TREE_ST, RSF_ANOMALY_DRAW_UNIDENTIFIED_DEFORMED) == 1);
    CHECK(cell_delta(&after, &before, RSF_ANOMALY_PASS_MAIN, RSF_ANOMALY_TYPE_TREE_ST, RSF_ANOMALY_DRAW_IDENTIFIED_FOLIAGE) == 0);

    rsf_anomaly_geometry_set_foliage(true);
    CHECK(rsf_anomaly_geometry_foliage());
    before = coverage_now();
    const rsf_anomaly_geometry_stats stats_before = stats_now();
    seen_count = 0;
    tree_st(tree_visual, 0.5f);
    tree_pm(tree_visual, 0.25f);
    after = coverage_now();
    CHECK(cell_delta(&after, &before, RSF_ANOMALY_PASS_MAIN, RSF_ANOMALY_TYPE_TREE_ST, RSF_ANOMALY_DRAW_IDENTIFIED_FOLIAGE) == 1);
    CHECK(cell_delta(&after, &before, RSF_ANOMALY_PASS_MAIN, RSF_ANOMALY_TYPE_TREE_PM, RSF_ANOMALY_DRAW_IDENTIFIED_FOLIAGE) == 1);
    CHECK(cell_delta(&after, &before, RSF_ANOMALY_PASS_MAIN, RSF_ANOMALY_TYPE_TREE_ST, RSF_ANOMALY_DRAW_UNIDENTIFIED_DEFORMED) == 0);
    CHECK(stats_now().draws_identified == stats_before.draws_identified + 2);
    CHECK(after.captured == before.captured); /* A foliage identity is not a registry capture. */
    CHECK(seen_count == 2);
    const uint64_t address = (uintptr_t)tree_visual;
    CHECK(seen_facts[0].in_visual && seen_facts[0].kind == RSF_ANOMALY_GEOMETRY_TREE && seen_facts[0].visual == address &&
        seen_facts[0].type == RSF_ANOMALY_TYPE_TREE_ST && seen_facts[0].tree_lod == -1 && !seen_facts[0].detail &&
        seen_facts[0].verdict == RSF_ANOMALY_DRAW_IDENTIFIED_FOLIAGE);
    CHECK(seen_facts[1].visual == address && seen_facts[1].type == RSF_ANOMALY_TYPE_TREE_PM &&
        seen_facts[1].tree_lod == 5);
    /* A shadow-map tree needs nothing. */
    set_u32(gp.phase, PHASE_SMAP);
    before = coverage_now();
    tree_st(tree_visual, 0.5f);
    after = coverage_now();
    CHECK(cell_delta(&after, &before, RSF_ANOMALY_PASS_SHADOW, RSF_ANOMALY_TYPE_TREE_ST, RSF_ANOMALY_DRAW_IGNORED_SHADOW) == 1);
    set_u32(gp.phase, PHASE_NORMAL);

    /* CDetailManager::Render: every flush inside it is a detail draw in its pass; outside it none is. */
    before = coverage_now();
    seen_count = 0;
    detail_render(&detail_calls);
    flush_target();
    after = coverage_now();
    CHECK(seen_count == 3 && seen_facts[0].detail && seen_facts[1].detail && !seen_facts[2].detail);
    CHECK(!seen_facts[0].in_visual && seen_facts[0].pass == RSF_ANOMALY_PASS_MAIN);
    CHECK(after.detail_renders[0] == before.detail_renders[0] + 1 &&
        after.flush_in_detail[0] == before.flush_in_detail[0] + 2);
    set_u32(gp.phase, PHASE_SMAP);
    detail_render(&detail_calls);
    set_u32(gp.phase, PHASE_NORMAL);
    const rsf_anomaly_geometry_coverage shadow = coverage_now();
    CHECK(shadow.detail_renders[1] == after.detail_renders[1] + 1 &&
        shadow.flush_in_detail[1] == after.flush_in_detail[1] + 2);
    rsf_anomaly_geometry_set_flush(NULL);
    rsf_anomaly_geometry_set_foliage(false);
    expect_identity = true;
}

static void run_vertex_constants(const void* unused)
{
    (void)unused;
    /* The engine's staged vertex constants: CBackend::m_aVertexConstants[slot] -> dx10ConstantBuffer { size +0x58,
     * data +0x60 }. Reads and writes are refused unless the object reports the reflected size. */
    unsigned char object[0x80] = {0};
    float staged[64];
    for (size_t i = 0; i < 64; ++i) staged[i] = (float)(i + 1);
    const uint32_t size = sizeof(staged);
    const uintptr_t data = (uintptr_t)staged;
    memcpy(object + CONSTANT_BUFFER_SIZE_OFFSET, &size, sizeof(size));
    memcpy(object + CONSTANT_BUFFER_DATA_OFFSET, &data, sizeof(data));
    const uintptr_t object_pointer = (uintptr_t)object;
    memcpy(module + gp.vertex_constants + 3 * sizeof(object_pointer), &object_pointer, sizeof(object_pointer));
    float out[4] = {0};
    CHECK(rsf_anomaly_geometry_read_constants(3, size, 16, out, sizeof(out)) && out[0] == 5.0f && out[3] == 8.0f);
    CHECK(!rsf_anomaly_geometry_read_constants(3, size + 16, 16, out, sizeof(out)));   /* reflected size differs */
    CHECK(!rsf_anomaly_geometry_read_constants(3, size, size - 8, out, sizeof(out)));  /* runs past the buffer */
    CHECK(!rsf_anomaly_geometry_read_constants(CONSTANT_BUFFER_SLOTS, size, 0, out, sizeof(out)));
    CHECK(!rsf_anomaly_geometry_read_constants(4, size, 0, out, sizeof(out)));          /* no buffer in that slot */
    const float poison[4] = {-1, -2, -3, -4};
    CHECK(rsf_anomaly_geometry_write_constants(3, size, 32, poison, sizeof(poison)) && staged[8] == -1.0f &&
        staged[11] == -4.0f && staged[7] == 8.0f && staged[12] == 13.0f);
    CHECK(!rsf_anomaly_geometry_write_constants(3, size + 4, 32, poison, sizeof(poison)) && staged[8] == -1.0f);
    float matrix[16] = {0};
    CHECK(rsf_anomaly_geometry_world_matrix(matrix));
}

static void set_viewport(bool active, uint8_t delay, uint32_t frame)
{
    memcpy(module + gp.viewport, &active, sizeof(active));
    memcpy(module + gp.viewport + SECOND_VIEWPORT_DELAY_OFFSET, &delay, sizeof(delay));
    memcpy(module + gp.frame, &frame, sizeof(frame));
}

static void run_contexts(const void* unused)
{
    (void)unused;
    /* Context producers: the engine's IsSVPFrame predicate selects the view, render_hud and the rmNear window the
     * first-person domain. */
    CHECK(rsf_anomaly_geometry_current_view(NULL, build) == UNSET);
    set_viewport(false, 2, 4);
    CHECK(rsf_anomaly_geometry_current_view(module, build) == MAIN);
    set_viewport(true, 2, 3);
    CHECK(rsf_anomaly_geometry_current_view(module, build) == MAIN);
    set_viewport(true, 2, 4);
    CHECK(rsf_anomaly_geometry_current_view(module, build) == SCOPE);
    set_viewport(true, 0, 4);
    CHECK(rsf_anomaly_geometry_current_view(module, build) == MAIN);
    const context outside = {UNSET, UNSET};
    CHECK(same_context(context_now(), outside));
    {
        set_viewport(true, 2, 4);
        rsf_anomaly_geometry_context view;
        CHECK(rsf_anomaly_geometry_render_view_enter(&view));
        const context world_scope = {SCOPE, WORLD}, hud_scope = {SCOPE, FIRST_PERSON};
        CHECK(same_context(context_now(), world_scope));
        hud(&hud_calls);
        CHECK(hud_calls == 1 && same_context(hud_context, hud_scope));
        CHECK(same_context(context_now(), world_scope));
        sorted(&hud_calls);
        CHECK(same_context(sorted_before, world_scope) && same_context(sorted_hud, hud_scope) &&
            same_context(sorted_after, world_scope));
        emissive(&hud_calls);
        CHECK(same_context(emissive_before, world_scope) && same_context(emissive_hud, hud_scope));
        CHECK(same_context(context_now(), world_scope)); /* The window closed on return although rmNormal never ran. */
        CHECK(near_calls == 2 && normal_calls == 1);
        near_target(&hud_calls); /* Outside sorted/emissive (particles, light volumes) rmNear opens nothing. */
        CHECK(same_context(context_now(), world_scope));
        normal_target(&hud_calls);
        rsf_anomaly_geometry_context_leave(&view);
    }
    CHECK(same_context(context_now(), outside));
    {
        set_viewport(true, 2, 4);
        rsf_anomaly_geometry_context view;
        CHECK(rsf_anomaly_geometry_render_view_enter(&view));
        const context world_scope = {SCOPE, WORLD}, hud_scope = {SCOPE, FIRST_PERSON};
        const context world_main = {MAIN, WORLD}, hud_main = {MAIN, FIRST_PERSON};
        const rsf_anomaly_geometry_coverage before = coverage_now();
        nested_window = true;
        sorted(&hud_calls);
        nested_window = false;
        CHECK(same_context(sorted_before, world_scope) && same_context(sorted_hud, hud_scope) &&
            same_context(sorted_after, world_scope));
        CHECK(same_context(emissive_before, world_main) && same_context(emissive_hud, hud_main) &&
            same_context(emissive_after, world_main));
        CHECK(same_context(context_now(), world_scope) && coverage_now().near_windows == before.near_windows + 2);

        rsf_anomaly_geometry_test_bind_module(NULL, build);
        {
            rsf_anomaly_geometry_context unavailable;
            CHECK(rsf_anomaly_geometry_render_view_enter(&unavailable));
            CHECK(same_context(context_now(), (context){UNSET, WORLD}));
            hud(&hud_calls);
            CHECK(same_context(hud_context, (context){UNSET, FIRST_PERSON}));
            rsf_anomaly_geometry_context_leave(&unavailable);
        }
        rsf_anomaly_geometry_test_bind_module(module, build);
        CHECK(same_context(context_now(), world_scope));
        rsf_anomaly_geometry_context_leave(&view);
    }
    CHECK(same_context(context_now(), outside));
    set_viewport(false, 2, 4);
    {
        rsf_anomaly_geometry_context view;
        CHECK(rsf_anomaly_geometry_render_view_enter(&view));
        CHECK(same_context(context_now(), (context){MAIN, WORLD}));
        rsf_anomaly_geometry_context_leave(&view);
    }
    {
        const rsf_anomaly_geometry_coverage coverage = coverage_now();
        CHECK(coverage.views_main >= 1 && coverage.views_scope >= 2 && coverage.hud_scopes >= 2 &&
            coverage.near_windows == 4);
    }
}

static bool has(const char* text)
{
    for (size_t i = 0; i < line_count; ++i)
        if (strstr(lines[i], text)) return true;
    return false;
}

static void run_coverage(const void* unused)
{
    (void)unused;
    line_count = 0;
    rsf_anomaly_geometry_report(collect_line, NULL);
    CHECK(has("coverage render-entries main=") && has("pass=main type=static identified=") &&
        has("pass=shadow type=static identified=") && has("pass=other type=static") &&
        has("draw-time flush main=") && has("captured=") && has("hud_scopes="));
    for (size_t i = 0; i < line_count; ++i) CHECK(strlen(lines[i]) < 600);
}

static void run_retirement(const void* unused)
{
    (void)unused;
    /* A borrowed child range does not belong to this owner, even when it cannot be traversed. */
    child_list[0] = NULL;
    const size_t borrowed_offset = HIERARCHY_CHILDREN_OFFSET + 3 * sizeof(void*);
    const int32_t borrowed_flag = 1;
    memcpy(visual + borrowed_offset, &borrowed_flag, sizeof(borrowed_flag));
    const rsf_anomaly_geometry_stats before_borrowed = stats_now();
    CHECK(create(&owner, expected_name, expected_reader) == visual);
    CHECK(stats_now().capture_faults == before_borrowed.capture_faults && rsf_anomaly_geometry_active());

    /* An owned hierarchy that cannot be walked latches capture off rather than publishing a partial lifetime. */
    const int32_t owned = 0;
    memcpy(visual + borrowed_offset, &owned, sizeof(owned));
    const rsf_anomaly_geometry_stats before_fault = stats_now();
    CHECK(create(&owner, expected_name, expected_reader) == visual);
    CHECK(stats_now().capture_faults == before_fault.capture_faults + 1 && !rsf_anomaly_geometry_active());
    CHECK(create(&owner, expected_name, expected_reader) == visual);
    CHECK(stats_now().models_registered == before_fault.models_registered);
    child_list[0] = child;
    /* With capture latched off a draw is not identified and says so; the flush hook stops observing. */
    {
        const rsf_anomaly_geometry_coverage before = coverage_now();
        expect_identity = false;
        render(child, 0.375f);
        const rsf_anomaly_geometry_coverage after = coverage_now();
        CHECK(cell_delta(&after, &before, RSF_ANOMALY_PASS_MAIN, RSF_ANOMALY_TYPE_STATIC, RSF_ANOMALY_DRAW_UNIDENTIFIED_NO_CAPTURE) == 1);
    }
}

static void run_release(const void* unused)
{
    (void)unused;
    CHECK(rsf_anomaly_hooks_disable(&hooks));
    rsf_anomaly_geometry_reset();
    rsf_anomaly_geometry_set_retired(0, NULL);
    const rsf_anomaly_geometry_stats stats = stats_now();
    CHECK(stats.models_registered >= 5 && stats.models_retired >= 5 && stats.draws_identified >= 3 &&
        stats.draws_unidentified >= 1 && stats.capture_faults == 1);
}

static void run_guards(const void* unused)
{
    (void)unused;
    unsigned char* image = allocate_fixture_image(gp.image_size);
    CHECK(rsf_anomaly_geometry_test_guard_lifecycle(image, build, other));
}

int main(int argc, char** argv)
{
    build = rsf_test_anomaly_select_build(argc, (const char* const*)argv);
    other = build ? 0 : 1;
    CHECK(rsf_test_anomaly_geometry_profile_get(build, &gp));
    rsf_test_anomaly_announce_build(gp.label, gp.executable);
    rsf_test_anomaly_geometry_profile other_profile;
    CHECK(rsf_test_anomaly_geometry_profile_get(other, &other_profile));
    /* One zeroed image large enough for either profile: a refused install must never read outside it. */
    const uint32_t image_bytes = gp.image_size > other_profile.image_size ? gp.image_size : other_profile.image_size;
    module = calloc(image_bytes, 1);
    CHECK(module);
    CHECK(!rsf_anomaly_geometry_install_native(NULL, build));
    CHECK(!rsf_anomaly_geometry_install_native(module, other));
    CHECK(!rsf_anomaly_geometry_install_native(module, build));
    CHECK(!rsf_anomaly_geometry_active());
    rsf_anomaly_geometry_test_bind_module(module, build);
    rsf_anomaly_geometry_set_retired(0, retired);
    memcpy(module + gp.world, world, sizeof(world));
    phase_at_flush = module + gp.phase;
    const void* table = module + gp.hierarchy_vtable;
    memcpy(visual, &table, sizeof(table));
    child_list[0] = child;
    const void* begin = child_list;
    const void* end = child_list + 1;
    memcpy(visual + 96, &begin, sizeof(begin));
    memcpy(visual + 104, &end, sizeof(end));
    memcpy(visual + 112, &end, sizeof(end));
    expected_owner = &owner;
    const rsf_anomaly_hook_site* sites = rsf_anomaly_geometry_hook_sites(build);
    CHECK(sites);
    unsigned char* base = (unsigned char*)GetModuleHandleW(NULL);
    const IMAGE_DOS_HEADER* dos = (const IMAGE_DOS_HEADER*)base;
    const IMAGE_NT_HEADERS64* pe = (const IMAGE_NT_HEADERS64*)(base + dos->e_lfanew);
    void* const targets[15] = {
        (void*)(uintptr_t)create_fixture, (void*)(uintptr_t)duplicate_fixture, (void*)(uintptr_t)delete_fixture,
        (void*)(uintptr_t)render_fixture, (void*)(uintptr_t)skin_st_fixture, (void*)(uintptr_t)skin_pm_fixture,
        (void*)(uintptr_t)flush_fixture, (void*)(uintptr_t)hud_fixture, (void*)(uintptr_t)sorted_fixture,
        (void*)(uintptr_t)emissive_fixture, (void*)(uintptr_t)near_fixture, (void*)(uintptr_t)normal_fixture,
        (void*)(uintptr_t)tree_st_fixture, (void*)(uintptr_t)tree_pm_fixture, (void*)(uintptr_t)detail_fixture,
    };
    memcpy(fixtures, targets, sizeof(fixtures));
    rsf_anomaly_hook_site redirects[15];
    CHECK(rsf_test_anomaly_geometry_redirects(base, pe->OptionalHeader.SizeOfImage, read_leaf, sites, SITE_COUNT,
        &fixtures, redirects));
    CHECK(rsf_anomaly_hooks_install(&hooks, base, pe->OptionalHeader.SizeOfImage, redirects, 15));
    create = (create_fn)fixtures[0];
    duplicate = (duplicate_fn)fixtures[1];
    destroy = (delete_fn)fixtures[2];
    render = (render_fn)fixtures[3];
    flush_target = (flush_fn)fixtures[6];
    hud = (unary_fn)fixtures[7];
    sorted = (unary_fn)fixtures[8];
    emissive = (unary_fn)fixtures[9];
    emissive_target = emissive;
    near_target = (unary_fn)fixtures[10];
    normal_target = (unary_fn)fixtures[11];
    tree_st = (render_fn)fixtures[12];
    tree_pm = (render_fn)fixtures[13];
    detail_render = (unary_fn)fixtures[14];
    const rsf_test_anomaly_geometry_hooks scenarios = {
        {NULL, run_lifecycle}, {NULL, run_flush_capture}, {NULL, run_phase}, {NULL, run_identities},
        {NULL, run_foliage}, {NULL, run_vertex_constants}, {NULL, run_contexts}, {NULL, run_coverage},
        {NULL, run_retirement}, {NULL, run_release}, {NULL, run_guards},
    };
    rsf_test_anomaly_geometry_run_hooks(&scenarios);

    /* Executable fixture allocations and trampolines intentionally live until process exit. */
    puts("PASS: actual x64 lifecycle/visual detours, RCX/RDX/R8/XMM1, hierarchy retirement before pooling/reuse, current world, "
        "draw-time flush capture and sink, pass/identity/static-world classification, view and first-person context producers, "
        "per-category coverage, full guarded production install/restore/reenable");
    return 0;
}
