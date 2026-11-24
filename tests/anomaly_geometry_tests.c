/* SPDX-License-Identifier: MIT */
/* Pure CPU contract of the Anomaly geometry owner: registry, thread-local scopes, real-layout world and palette
 * capture, classification, context scopes and the feed into the Rust temporal history. No engine code runs. */
#include "native_geometry.h"
#include "native_geometry/fixture_platform.h"
#include "../games/anomaly/temporal/fixture_platform.h"
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <windows.h>

#define CHECK(expression) do { if (!(expression)) { fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #expression); exit(1); } } while (0)

enum { UNSET = RSF_ANOMALY_GEOMETRY_UNSET };
enum { MAIN = RSF_ANOMALY_GEOMETRY_MAIN, SCOPE = RSF_ANOMALY_GEOMETRY_SCOPE };
enum { WORLD = RSF_ANOMALY_GEOMETRY_WORLD, FIRST_PERSON = RSF_ANOMALY_GEOMETRY_FIRST_PERSON };
enum {
    KIND_RIGID = RSF_ANOMALY_GEOMETRY_RIGID, KIND_HARDWARE_SKINNED = RSF_ANOMALY_GEOMETRY_HARDWARE_SKINNED,
    KIND_SOFTWARE_SKINNED = RSF_ANOMALY_GEOMETRY_SOFTWARE_SKINNED, KIND_TREE = RSF_ANOMALY_GEOMETRY_TREE,
    KIND_SINGLE_BONE = RSF_ANOMALY_GEOMETRY_SINGLE_BONE,
};
/* temporal::CaptureStatus and BeginStatus of the Rust history fixture. */
enum { CAPTURED = 0, REPEATED = 1, CONFLICT = 4, RETIRED = 5, MISSING_PROJECTION = 6 };
enum { STARTED = 0 };
/* The native feed statuses behind rsf_anomaly_geometry_snapshot_kind. */
enum {
    FEED_NO_DRAW, FEED_MISSING_CONTEXT, FEED_WRONG_VIEW, FEED_MISSING_WORLD, FEED_UNSUPPORTED_KIND,
    FEED_MISSING_PALETTE, FEED_REJECTED, FEED_CAPTURED,
};
enum { INSTANCE_RIGID = 0, INSTANCE_SKINNED = 1 };

uint8_t rsf_anomaly_geometry_snapshot_kind(const rsf_anomaly_geometry_snapshot*, uint8_t*);

_Static_assert(sizeof(rsf_anomaly_plan_key) == sizeof(rsf_anomaly_geometry_key) &&
    sizeof(rsf_anomaly_plan_matrix) == sizeof(rsf_anomaly_geometry_matrix), "history and geometry records match");

typedef struct feed_result {
    int status;
    bool has_history;
    uint32_t history;
} feed_result;

typedef struct frame_token {
    uint8_t view;
    rsf_anomaly_plan_token native;
} frame_token;

/* A copy of one capture that owns its palette. */
typedef struct draw_copy {
    bool present;
    rsf_anomaly_geometry_key key;
    bool has_world;
    rsf_anomaly_geometry_matrix world;
    rsf_anomaly_geometry_matrix* bones;
    size_t bone_count;
    uint64_t palette_layout;
    uint8_t view, domain, kind;
} draw_copy;

static void copy_release(draw_copy* copy)
{
    free(copy->bones);
    memset(copy, 0, sizeof(*copy));
}

static void copy_from(draw_copy* copy, const rsf_anomaly_geometry_snapshot* snapshot)
{
    memset(copy, 0, sizeof(*copy));
    copy->present = true;
    copy->key = snapshot->key;
    copy->has_world = snapshot->world != NULL;
    if (snapshot->world) copy->world = *snapshot->world;
    copy->palette_layout = snapshot->palette_layout;
    copy->view = snapshot->view;
    copy->domain = snapshot->domain;
    copy->kind = snapshot->draw_kind;
    if (snapshot->bone_count) {
        copy->bones = malloc(snapshot->bone_count * sizeof(*copy->bones));
        CHECK(copy->bones);
        memcpy(copy->bones, snapshot->bones, snapshot->bone_count * sizeof(*copy->bones));
        copy->bone_count = snapshot->bone_count;
    }
}

/* The live draw of this thread, copied: the equivalent of the former capture_current_draw. */
static bool capture_copy(draw_copy* copy)
{
    rsf_anomaly_geometry_capture_storage storage;
    const rsf_anomaly_geometry_snapshot* snapshot = rsf_anomaly_geometry_capture_current(&storage);
    memset(copy, 0, sizeof(*copy));
    if (!snapshot) return false;
    copy_from(copy, snapshot);
    rsf_anomaly_geometry_capture_release(&storage);
    return true;
}

/* DrawScope with the registry key of an acquired identity. */
typedef rsf_anomaly_geometry_visual scope;

static void scope_enter(scope* s, const rsf_anomaly_geometry_registry* registry, bool keyed,
    rsf_anomaly_geometry_key key, uint8_t kind, const float* world, const uint8_t* skeleton, uint8_t pass,
    uint8_t type, uint8_t verdict)
{
    memset(s, 0, sizeof(*s));
    s->registry = registry;
    if (keyed) s->key = key;
    s->world = (uintptr_t)world;
    s->skeleton = (uintptr_t)skeleton;
    s->kind = kind;
    s->pass = pass;
    s->type = type;
    s->verdict = verdict;
    rsf_anomaly_geometry_visual_enter(s);
}

static void draw_enter(scope* s, const rsf_anomaly_geometry_registry* registry, bool keyed,
    rsf_anomaly_geometry_key key, uint8_t kind, const float* world, const uint8_t* skeleton)
{
    scope_enter(s, registry, keyed, key, kind, world, skeleton, RSF_ANOMALY_PASS_MAIN, RSF_ANOMALY_TYPE_STATIC,
        RSF_ANOMALY_DRAW_IDENTIFIED);
}

static void draw_leave(scope* s) { rsf_anomaly_geometry_visual_leave(s); }

static const rsf_anomaly_geometry_visual* borrowed(void)
{
    return (const rsf_anomaly_geometry_visual*)rsf_anomaly_geometry_borrow_draw();
}

typedef rsf_anomaly_geometry_context view_scope;

static void view_enter(view_scope* v, uint8_t view, uint8_t domain)
{
    rsf_anomaly_geometry_context_enter(v, view, domain, false);
}

static void inherit_enter(view_scope* v, uint8_t domain)
{
    rsf_anomaly_geometry_context_enter(v, UNSET, domain, true);
}

static void view_leave(view_scope* v) { rsf_anomaly_geometry_context_leave(v); }

static bool same_key(const rsf_anomaly_geometry_key* a, const rsf_anomaly_geometry_key* b)
{
    return a->owner == b->owner && a->visual == b->visual && a->generation == b->generation;
}

static bool acquire(rsf_anomaly_geometry_registry* registry, uint64_t owner, uint64_t visual,
    rsf_anomaly_geometry_key* key)
{
    const uint64_t generation = rsf_anomaly_geometry_register(registry, owner, visual);
    if (!generation) return false;
    *key = (rsf_anomaly_geometry_key){owner, visual, generation};
    return true;
}

static void retire_identity(rsf_anomaly_geometry_registry* registry, uint64_t owner, uint64_t visual)
{
    (void)rsf_anomaly_geometry_retire(registry, owner, visual);
}

static void store(void* buffer, size_t offset, const void* value, size_t size)
{
    memcpy((unsigned char*)buffer + offset, value, size);
}

/* The retirement queue and its history sink. */
static void retire_in_history(void* history, const rsf_anomaly_geometry_key* key)
{
    (void)rsf_anomaly_plan_history_retire(history, (const rsf_anomaly_plan_key*)key);
}

static size_t drain_history(rsf_anomaly_geometry_retirements* queue, void* history)
{
    return rsf_anomaly_geometry_drain(queue, retire_in_history, history);
}

static void sink(rsf_anomaly_geometry_retirements* queue, const rsf_anomaly_geometry_key* key)
{
    if (queue) rsf_anomaly_geometry_enqueue(queue, key);
}

/* A hardware or rigid copy as the temporal history receives it. */
static int to_pose(const draw_copy* draw, rsf_anomaly_plan_pose* pose, rsf_anomaly_geometry_snapshot* facts)
{
    uint8_t kind = 0;
    memset(facts, 0, sizeof(*facts));
    facts->key = draw->key;
    facts->world = draw->has_world ? &draw->world : NULL;
    facts->bones = draw->bones;
    facts->bone_count = draw->bone_count;
    facts->palette_layout = draw->palette_layout;
    facts->view = draw->view;
    facts->domain = draw->domain;
    facts->draw_kind = draw->kind;
    const int why = rsf_anomaly_geometry_snapshot_kind(facts, &kind);
    if (why != FEED_CAPTURED) return why;
    memset(pose, 0, sizeof(*pose));
    memcpy(&pose->world, draw->world.values, sizeof(pose->world));
    pose->domain = draw->domain;
    pose->kind = kind;
    if (kind == INSTANCE_SKINNED) {
        pose->palette_layout = draw->palette_layout;
        pose->bones = (const rsf_anomaly_plan_matrix*)draw->bones;
        pose->bone_count = draw->bone_count;
    }
    return FEED_CAPTURED;
}

static feed_result feed_draw(void* history, const frame_token* frame, rsf_anomaly_geometry_retirements* queue,
    const draw_copy* draw)
{
    drain_history(queue, history);
    if (draw->view != UNSET && draw->view != frame->view) return (feed_result){FEED_WRONG_VIEW, false, 0};
    rsf_anomaly_plan_pose pose;
    rsf_anomaly_geometry_snapshot facts;
    const int why = to_pose(draw, &pose, &facts);
    if (why != FEED_CAPTURED) return (feed_result){why, false, 0};
    rsf_anomaly_plan_capture out;
    memset(&out, 0, sizeof(out));
    rsf_anomaly_plan_history_capture(history, &frame->native, (const rsf_anomaly_plan_key*)&draw->key, &pose, &out);
    const bool ok = out.status == CAPTURED || out.status == REPEATED;
    return (feed_result){ok ? FEED_CAPTURED : FEED_REJECTED, true, out.status};
}

static feed_result feed_current_draw(void* history, const frame_token* frame,
    rsf_anomaly_geometry_retirements* queue)
{
    draw_copy draw;
    if (!capture_copy(&draw)) {
        drain_history(queue, history);
        return (feed_result){FEED_NO_DRAW, false, 0};
    }
    const feed_result result = feed_draw(history, frame, queue, &draw);
    copy_release(&draw);
    return result;
}

static float world[16] = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 7, 8, 9, 1};
static unsigned char parent[256], skeleton[64], bones[336];
static rsf_anomaly_geometry_registry* main_registry;
static rsf_anomaly_geometry_key live;

static DWORD WINAPI isolated_worker(void* result)
{
    draw_copy none;
    *(bool*)result = !borrowed() && !capture_copy(&none);
    return 0;
}

static void run_capture(const void* unused)
{
    (void)unused;
    CHECK(!borrowed());
    {
        scope draw;
        draw_enter(&draw, main_registry, true, live, KIND_RIGID, world, NULL);
        CHECK(borrowed() && same_key(&borrowed()->key, &live));
        CHECK(borrowed()->view == UNSET && borrowed()->domain == UNSET);
        draw_copy copy;
        CHECK(capture_copy(&copy) && copy.has_world && copy.world.values[3] == 7);
        CHECK(copy.bone_count == 0 && copy.palette_layout == 0);
        world[12] = 17;
        draw_copy later;
        CHECK(capture_copy(&later) && later.world.values[3] == 17); /* Sample at draw, not visual entry. */
        CHECK(copy.world.values[3] == 7); /* Copy is not an engine lifetime lease. */
        copy_release(&copy);
        copy_release(&later);
        {
            scope unmatched;
            draw_enter(&unmatched, main_registry, false, (rsf_anomaly_geometry_key){0}, KIND_RIGID, NULL, NULL);
            draw_copy none;
            CHECK(!borrowed() && !capture_copy(&none));
            draw_leave(&unmatched);
        }
        CHECK(borrowed() && same_key(&borrowed()->key, &live));
        retire_identity(main_registry, live.owner, live.visual);
        CHECK(!borrowed()); /* An active scope is invalidated by retirement. */
        draw_leave(&draw);
    }
    CHECK(!borrowed());
    rsf_anomaly_geometry_key contextual;
    CHECK(acquire(main_registry, 0x10, 0x40, &contextual));
    {
        view_scope frame;
        view_enter(&frame, SCOPE, WORLD);
        scope draw;
        draw_enter(&draw, main_registry, true, contextual, KIND_RIGID, world, NULL);
        CHECK(borrowed()->view == SCOPE && borrowed()->domain == WORLD);
        {
            view_scope hud;
            view_enter(&hud, SCOPE, FIRST_PERSON);
            scope hands;
            draw_enter(&hands, main_registry, true, contextual, KIND_RIGID, world, NULL);
            CHECK(borrowed()->domain == FIRST_PERSON);
            /* Another thread sees no scope of this thread. */
            bool isolated = false;
            HANDLE other = CreateThread(NULL, 0, isolated_worker, &isolated, 0, NULL);
            CHECK(other);
            WaitForSingleObject(other, INFINITE);
            CloseHandle(other);
            CHECK(isolated);
            draw_leave(&hands);
            view_leave(&hud);
        }
        CHECK(borrowed()->domain == WORLD);
        draw_leave(&draw);
        view_leave(&frame);
    }
    {
        scope no_world;
        draw_enter(&no_world, main_registry, true, contextual, KIND_RIGID, NULL, NULL);
        draw_copy copy;
        CHECK(capture_copy(&copy) && !copy.has_world);
        copy_release(&copy);
        CHECK(borrowed()->view == UNSET && borrowed()->domain == UNSET);
        draw_leave(&no_world);
    }
    world[0] = NAN;
    {
        scope invalid;
        draw_enter(&invalid, main_registry, true, contextual, KIND_RIGID, world, NULL);
        draw_copy copy;
        CHECK(capture_copy(&copy) && !copy.has_world);
        copy_release(&copy);
        draw_leave(&invalid);
    }
    world[0] = 1;
    const unsigned char* parent_pointer = parent;
    const unsigned char* bone_pointer = bones;
    store(skeleton, 8, &parent_pointer, sizeof(parent_pointer));
    const uint16_t mode = 2;
    store(skeleton, 56, &mode, sizeof(mode));
    uint32_t count = 2;
    store(skeleton, 60, &count, sizeof(count));
    store(parent, 208, &bone_pointer, sizeof(bone_pointer));
    memcpy(bones + 64, world, sizeof(world));
    world[12] = 37;
    memcpy(bones + 168 + 64, world, sizeof(world));
    {
        scope skin;
        draw_enter(&skin, main_registry, true, contextual, KIND_HARDWARE_SKINNED, world, skeleton);
        draw_copy copy;
        CHECK(capture_copy(&copy) && copy.bone_count == 2 && copy.palette_layout != 0);
        CHECK(copy.bones[0].values[3] == 17 && copy.bones[1].values[3] == 37);
        copy_release(&copy);
        const uintptr_t wrapping_pointer = UINTPTR_MAX - 64;
        store(parent, 208, &wrapping_pointer, sizeof(wrapping_pointer));
        draw_copy unreadable;
        CHECK(capture_copy(&unreadable) && unreadable.has_world && unreadable.bone_count == 0 &&
            unreadable.palette_layout == 0);
        copy_release(&unreadable);
        store(parent, 208, &bone_pointer, sizeof(bone_pointer));
        count = 1025;
        store(skeleton, 60, &count, sizeof(count));
        draw_copy over;
        CHECK(capture_copy(&over) && over.bone_count == 0 && over.palette_layout == 0);
        copy_release(&over);
        const uint16_t software = 0;
        store(skeleton, 56, &software, sizeof(software));
        draw_copy soft;
        CHECK(capture_copy(&soft) && soft.bone_count == 0);
        copy_release(&soft);
        draw_leave(&skin);
    }
}

static rsf_anomaly_plan_frame make_frame(uint8_t view, uint64_t source)
{
    rsf_anomaly_plan_frame frame;
    memset(&frame, 0, sizeof(frame));
    frame.view = view;
    frame.source_frame = source;
    for (size_t i = 0; i < 4; ++i) {
        frame.world.view.values[i * 4 + i] = 1;
        frame.world.projection.values[i * 4 + i] = 1;
    }
    frame.sizes = (rsf_anomaly_plan_resolutions){{1920, 1080}, {1280, 720}, {1024, 512}};
    return frame;
}

static bool begin_frame(void* history, uint8_t view, uint64_t source, frame_token* token)
{
    rsf_anomaly_plan_frame frame = make_frame(view, source);
    memset(token, 0, sizeof(*token));
    const uint32_t status = rsf_anomaly_plan_history_begin(history, &frame, &token->native);
    token->view = (uint8_t)token->native.view;
    return status == STARTED && token->native.owner != 0;
}

static rsf_anomaly_plan_capture capture_pose(void* history, const frame_token* token,
    const rsf_anomaly_geometry_key* key, const rsf_anomaly_plan_pose* pose)
{
    rsf_anomaly_plan_capture out;
    memset(&out, 0, sizeof(out));
    rsf_anomaly_plan_history_capture(history, &token->native, (const rsf_anomaly_plan_key*)key, pose, &out);
    return out;
}

typedef struct worker_state {
    rsf_anomaly_geometry_retirements* queue;
    rsf_anomaly_geometry_key key;
} worker_state;

static DWORD WINAPI lifecycle_worker(void* context)
{
    worker_state* state = context;
    sink(state->queue, &state->key);
    sink(NULL, &state->key);
    return 0;
}

typedef struct requeue_state {
    rsf_anomaly_geometry_retirements* queue;
    unsigned seen;
} requeue_state;

static void requeue(void* user, const rsf_anomaly_geometry_key* key)
{
    requeue_state* state = user;
    ++state->seen;
    sink(state->queue, key);
}

static void run_history(const void* unused)
{
    (void)unused;
    float feed_world[16] = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 3, 4, 5, 1};
    rsf_anomaly_geometry_registry* feed = rsf_anomaly_geometry_registry_create();
    rsf_anomaly_geometry_key rigid_key, skin_key;
    CHECK(acquire(feed, 0x1, 0x100, &rigid_key) && acquire(feed, 0x1, 0x200, &skin_key));
    void* history = rsf_anomaly_plan_history_create();
    rsf_anomaly_geometry_retirements* queue = rsf_anomaly_geometry_retirements_create();
    frame_token first;
    CHECK(begin_frame(history, MAIN, 10, &first));
    CHECK(feed_current_draw(history, &first, queue).status == FEED_NO_DRAW);
    {
        scope draw;
        draw_enter(&draw, feed, true, rigid_key, KIND_RIGID, feed_world, NULL);
        CHECK(feed_current_draw(history, &first, queue).status == FEED_MISSING_CONTEXT);
        draw_leave(&draw);
    }
    {
        view_scope view;
        view_enter(&view, SCOPE, WORLD);
        scope draw;
        draw_enter(&draw, feed, true, rigid_key, KIND_RIGID, feed_world, NULL);
        CHECK(feed_current_draw(history, &first, queue).status == FEED_WRONG_VIEW);
        draw_leave(&draw);
        view_leave(&view);
    }
    {
        view_scope view;
        view_enter(&view, MAIN, WORLD);
        scope draw;
        draw_enter(&draw, feed, true, rigid_key, KIND_RIGID, feed_world, NULL);
        feed_result fed = feed_current_draw(history, &first, queue);
        CHECK(fed.status == FEED_CAPTURED && fed.has_history && fed.history == CAPTURED);
        CHECK(feed_current_draw(history, &first, queue).history == REPEATED);
        feed_world[12] = 99; /* A different world inside the same frame must conflict, never overwrite. */
        const feed_result conflict = feed_current_draw(history, &first, queue);
        CHECK(conflict.status == FEED_REJECTED && conflict.history == CONFLICT);
        feed_world[12] = 3;
        /* The first-person domain has no projection in this frame, so history refuses it. */
        view_scope hands;
        view_enter(&hands, MAIN, FIRST_PERSON);
        scope held;
        draw_enter(&held, feed, true, rigid_key, KIND_RIGID, feed_world, NULL);
        const feed_result refused = feed_current_draw(history, &first, queue);
        CHECK(refused.status == FEED_REJECTED && refused.history == MISSING_PROJECTION);
        draw_leave(&held);
        view_leave(&hands);
        draw_leave(&draw);
        view_leave(&view);
    }
    for (int pass = 0; pass < 2; ++pass) {
        view_scope view;
        view_enter(&view, MAIN, WORLD);
        scope draw;
        draw_enter(&draw, feed, true, rigid_key, pass ? KIND_TREE : KIND_SOFTWARE_SKINNED, feed_world, NULL);
        CHECK(feed_current_draw(history, &first, queue).status == FEED_UNSUPPORTED_KIND);
        draw_leave(&draw);
        view_leave(&view);
    }
    {
        /* RM_SINGLE: the world read at the draw is the bone-composed one, so the snapshot is a rigid world, no palette. */
        draw_copy single;
        memset(&single, 0, sizeof(single));
        single.key = rigid_key;
        single.kind = KIND_SINGLE_BONE;
        single.view = MAIN;
        single.domain = WORLD;
        single.has_world = true;
        for (size_t i = 0; i < 4; ++i) single.world.values[i * 4 + i] = 1;
        single.world.values[3] = 2;
        rsf_anomaly_plan_pose pose;
        rsf_anomaly_geometry_snapshot facts;
        CHECK(to_pose(&single, &pose, &facts) == FEED_CAPTURED && pose.kind == INSTANCE_RIGID &&
            pose.bone_count == 0 && pose.palette_layout == 0 && pose.world.values[3] == 2);
        single.has_world = false;
        CHECK(to_pose(&single, &pose, &facts) == FEED_MISSING_WORLD);
    }
    {
        view_scope view;
        view_enter(&view, MAIN, WORLD);
        scope draw;
        draw_enter(&draw, feed, true, rigid_key, KIND_RIGID, NULL, NULL);
        CHECK(feed_current_draw(history, &first, queue).status == FEED_MISSING_WORLD);
        draw_leave(&draw);
        view_leave(&view);
    }
    /* Hardware skinning, using the layout buffers built above (two bones at +64 with stride 168). */
    uint16_t mode = 2;
    uint32_t count = 2;
    store(skeleton, 56, &mode, sizeof(mode));
    store(skeleton, 60, &count, sizeof(count));
    {
        view_scope view;
        view_enter(&view, MAIN, WORLD);
        scope draw;
        draw_enter(&draw, feed, true, skin_key, KIND_HARDWARE_SKINNED, feed_world, skeleton);
        const feed_result fed = feed_current_draw(history, &first, queue);
        CHECK(fed.status == FEED_CAPTURED && fed.has_history && fed.history == CAPTURED);
        count = 0; /* Unreadable palette must not become an invented identity pose. */
        store(skeleton, 60, &count, sizeof(count));
        CHECK(feed_current_draw(history, &first, queue).status == FEED_MISSING_PALETTE);
        count = 2;
        store(skeleton, 60, &count, sizeof(count));
        draw_leave(&draw);
        view_leave(&view);
    }
    /* The next source frame links the previous snapshot by identity; a recycled generation does not. */
    frame_token second;
    CHECK(begin_frame(history, MAIN, 11, &second));
    {
        view_scope view;
        view_enter(&view, MAIN, WORLD);
        scope draw;
        draw_enter(&draw, feed, true, rigid_key, KIND_RIGID, feed_world, NULL);
        draw_copy capture;
        CHECK(capture_copy(&capture));
        rsf_anomaly_plan_pose pose;
        rsf_anomaly_geometry_snapshot facts;
        CHECK(to_pose(&capture, &pose, &facts) == FEED_CAPTURED);
        const rsf_anomaly_plan_capture linked = capture_pose(history, &second, &capture.key, &pose);
        CHECK(linked.status == CAPTURED && linked.has_previous && linked.previous_world.values[3] == 3);
        copy_release(&capture);
        draw_leave(&draw);
        view_leave(&view);
    }
    /* Retirement crosses threads through the queue and is applied by the history owner. */
    worker_state state = {queue, rigid_key};
    HANDLE lifecycle = CreateThread(NULL, 0, lifecycle_worker, &state, 0, NULL);
    CHECK(lifecycle);
    WaitForSingleObject(lifecycle, INFINITE);
    CloseHandle(lifecycle);
    retire_identity(feed, rigid_key.owner, rigid_key.visual);
    CHECK(rsf_anomaly_geometry_dropped(queue) == 0);
    CHECK(drain_history(queue, history) == 1 && drain_history(queue, history) == 0);
    sink(queue, &rigid_key);
    requeue_state requeued = {queue, 0};
    CHECK(rsf_anomaly_geometry_drain(queue, requeue, &requeued) == 1);
    CHECK(requeued.seen == 1 && drain_history(queue, history) == 1);
    rsf_anomaly_plan_pose retired_pose;
    memset(&retired_pose, 0, sizeof(retired_pose));
    for (size_t i = 0; i < 4; ++i) retired_pose.world.values[i * 4 + i] = 1;
    CHECK(capture_pose(history, &second, &rigid_key, &retired_pose).status == RETIRED);
    rsf_anomaly_geometry_key recycled;
    CHECK(acquire(feed, 0x1, 0x100, &recycled) && recycled.generation > rigid_key.generation);
    {
        view_scope view;
        view_enter(&view, MAIN, WORLD);
        scope draw;
        draw_enter(&draw, feed, true, recycled, KIND_RIGID, feed_world, NULL);
        const feed_result fed = feed_current_draw(history, &second, queue);
        CHECK(fed.status == FEED_CAPTURED && fed.has_history && fed.history == CAPTURED);
        draw_leave(&draw);
        view_leave(&view);
    }
    rsf_anomaly_geometry_stats stats;
    rsf_anomaly_geometry_stats_read(&stats);
    CHECK(stats.capture_faults == 0);
    rsf_anomaly_geometry_retirements_destroy(queue);
    rsf_anomaly_plan_history_destroy(history);
    rsf_anomaly_geometry_registry_destroy(feed);
}

static void run_coverage(const void* unused)
{
    (void)unused;
    /* Verify the native name adapters still expose the Rust coverage labels. */
    for (uint8_t i = 0; i < RSF_ANOMALY_DRAW_COUNT; ++i) CHECK(strlen(rsf_anomaly_geometry_verdict_name(i)) > 3);
    for (uint8_t i = 0; i < RSF_ANOMALY_TYPE_COUNT; ++i) CHECK(strlen(rsf_anomaly_geometry_type_name(i)) > 3);
    for (uint8_t i = 0; i < RSF_ANOMALY_PASS_COUNT; ++i) CHECK(strlen(rsf_anomaly_geometry_pass_name(i)) > 3);
}

static void run_inheritance(const void* unused)
{
    (void)unused;
    /* The enclosing view survives a projection-domain change, and a classification travels with the draw. */
    rsf_anomaly_geometry_registry* scoped = rsf_anomaly_geometry_registry_create();
    rsf_anomaly_geometry_key key;
    CHECK(acquire(scoped, 0x9, 0x900, &key));
    {
        view_scope main_frame;
        view_enter(&main_frame, SCOPE, WORLD);
        {
            view_scope hud_window;
            inherit_enter(&hud_window, FIRST_PERSON);
            scope draw;
            scope_enter(&draw, scoped, true, key, KIND_RIGID, world, NULL, RSF_ANOMALY_PASS_SHADOW,
                RSF_ANOMALY_TYPE_PROGRESSIVE, RSF_ANOMALY_DRAW_IGNORED_SHADOW);
            CHECK(borrowed()->view == SCOPE && borrowed()->domain == FIRST_PERSON);
            CHECK(borrowed()->pass == RSF_ANOMALY_PASS_SHADOW);
            CHECK(borrowed()->type == RSF_ANOMALY_TYPE_PROGRESSIVE);
            CHECK(borrowed()->verdict == RSF_ANOMALY_DRAW_IGNORED_SHADOW);
            draw_leave(&draw);
            view_leave(&hud_window);
        }
        scope after;
        draw_enter(&after, scoped, true, key, KIND_RIGID, world, NULL);
        CHECK(borrowed()->domain == WORLD);
        CHECK(borrowed()->verdict == RSF_ANOMALY_DRAW_IDENTIFIED && borrowed()->pass == RSF_ANOMALY_PASS_MAIN);
        draw_leave(&after);
        view_leave(&main_frame);
    }
    {
        /* With no enclosing frame there is nothing to inherit; the draw keeps no view. */
        view_scope orphan;
        inherit_enter(&orphan, FIRST_PERSON);
        scope draw;
        draw_enter(&draw, scoped, true, key, KIND_RIGID, world, NULL);
        CHECK(borrowed()->view == UNSET && borrowed()->domain == FIRST_PERSON);
        draw_leave(&draw);
        view_leave(&orphan);
    }
    rsf_anomaly_geometry_registry_destroy(scoped);
}

static void run_sink(const void* unused)
{
    (void)unused;
    /* The draw sink adaptor feeds an already-copied capture; a foreign view is refused like a live draw. */
    rsf_anomaly_geometry_retirements* queue = rsf_anomaly_geometry_retirements_create();
    void* history = rsf_anomaly_plan_history_create();
    frame_token frame;
    CHECK(begin_frame(history, MAIN, 40, &frame));
    rsf_anomaly_geometry_registry* ids = rsf_anomaly_geometry_registry_create();
    rsf_anomaly_geometry_key key;
    CHECK(acquire(ids, 0x1, 0x700, &key));
    float sink_world[16] = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 5, 6, 7, 1};
    draw_copy copy;
    {
        view_scope view;
        view_enter(&view, MAIN, WORLD);
        scope draw;
        draw_enter(&draw, ids, true, key, KIND_RIGID, sink_world, NULL);
        CHECK(capture_copy(&copy));
        draw_leave(&draw);
        view_leave(&view);
    }
    feed_result fed = feed_draw(history, &frame, queue, &copy);
    CHECK(fed.status == FEED_CAPTURED && fed.has_history && fed.history == CAPTURED);
    CHECK(feed_draw(history, &frame, queue, &copy).history == REPEATED);
    draw_copy foreign = copy;
    foreign.bones = NULL;
    foreign.bone_count = 0;
    foreign.view = SCOPE;
    CHECK(feed_draw(history, &frame, queue, &foreign).status == FEED_WRONG_VIEW);
    foreign.view = UNSET;
    CHECK(feed_draw(history, &frame, queue, &foreign).status == FEED_MISSING_CONTEXT);
    draw_copy tree = foreign;
    tree.view = copy.view;
    tree.kind = KIND_TREE;
    CHECK(feed_draw(history, &frame, queue, &tree).status == FEED_UNSUPPORTED_KIND);
    /* Retirements queued by the engine thread still drain through the sink adaptor. */
    sink(queue, &key);
    CHECK(feed_draw(history, &frame, queue, &copy).history == RETIRED);
    copy_release(&copy);
    rsf_anomaly_geometry_registry_destroy(ids);
    rsf_anomaly_plan_history_destroy(history);
    rsf_anomaly_geometry_retirements_destroy(queue);
}

int main(void)
{
    main_registry = rsf_anomaly_geometry_registry_create();
    CHECK(acquire(main_registry, 0x10, 0x20, &live));
    CHECK(rsf_anomaly_geometry_find(main_registry, 0x10, 0x20) == live.generation);
    const rsf_test_anomaly_geometry_contract scenarios = {
        {NULL, run_capture}, {NULL, run_history}, {NULL, run_coverage}, {NULL, run_inheritance}, {NULL, run_sink},
    };
    rsf_test_anomaly_geometry_run_contract(&scenarios);
    rsf_anomaly_geometry_stats stats;
    rsf_anomaly_geometry_stats_read(&stats);
    CHECK(stats.capture_faults == 0);
    rsf_anomaly_geometry_registry_destroy(main_registry);
    puts("Anomaly geometry registry, TLS, real-layout world/palette capture, classification, context scopes, history feed: PASS");
    return 0;
}
