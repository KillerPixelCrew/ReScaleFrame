// SPDX-License-Identifier: GPL-3.0-only
#include "native_geometry.h"
#include "geometry_feed.h"
#include <cstdio>
#include <cstdlib>
#include <thread>
#include <limits>
#include <cstring>
#include <vector>

#define CHECK(expression) do { if (!(expression)) { std::fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #expression); std::exit(1); } } while (false)
using namespace rsf::anomaly::geometry;

int main()
{
    IdentityRegistry identities;
    CHECK(!identities.find(0x10, 0x20));
    const auto created = identities.acquire(0x10, 0x20);
    CHECK(created && created->owner == 0x10 && created->visual == 0x20 && created->generation != 0);
    CHECK(identities.find(0x10, 0x20) == created);
    CHECK(!identities.acquire(0, 0x20));
    CHECK(!identities.acquire(0x10, 0));
    CHECK(!identities.acquire(0x10, 0x20)); // A second create cannot silently preserve a live lifetime.
    CHECK(identities.retire(0x10, 0x20) == created);
    CHECK(!identities.find(0x10, 0x20));
    CHECK(!identities.retire(0x10, 0x20));
    const auto recycled = identities.acquire(0x10, 0x20);
    CHECK(recycled && recycled->generation > created->generation);
    CHECK(identities.find_visual(0x20) == recycled);
    CHECK(identities.acquire(0x30, 0x20));
    CHECK(!identities.find_visual(0x20)); // An ambiguous pool never supplies a draw identity.
    const auto retired = identities.retire_all();
    CHECK(retired.size() == 2 && !identities.find_visual(0x20));
    CHECK(identities.acquire(0x10, 0x20)->generation > recycled->generation);
    const auto live = identities.find(0x10, 0x20);
    float world[16]{1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 7, 8, 9, 1};
    CHECK(!borrow_draw());
    {
        DrawScope scope(identities, live, DrawKind::Rigid, world);
        CHECK(borrow_draw() && borrow_draw()->key == *live);
        CHECK(!borrow_draw()->view && !borrow_draw()->domain);
        const auto copy = capture_current_draw();
        CHECK(copy && copy->world && copy->world->at(0, 3) == 7);
        CHECK(copy->bones.empty() && copy->palette_layout == 0);
        world[12] = 17;
        CHECK(capture_current_draw()->world->at(0, 3) == 17); // Sample at draw, not visual entry.
        CHECK(copy->world->at(0, 3) == 7); // Copy is not an engine lifetime lease.
        {
            DrawScope unmatched(identities, {}, DrawKind::Rigid, nullptr);
            CHECK(!borrow_draw() && !capture_current_draw());
        }
        CHECK(borrow_draw() && borrow_draw()->key == *live);
        identities.retire(live->owner, live->visual);
        CHECK(!borrow_draw()); // An active scope is invalidated by retirement.
    }
    CHECK(!borrow_draw());
    const auto contextual_key = identities.acquire(0x10, 0x40);
    {
        DrawContextScope view(rsf::anomaly::temporal::ViewId::Scope, rsf::anomaly::temporal::ProjectionDomain::World);
        DrawScope draw(identities, contextual_key, DrawKind::Rigid, world);
        CHECK(borrow_draw()->view == rsf::anomaly::temporal::ViewId::Scope);
        CHECK(borrow_draw()->domain == rsf::anomaly::temporal::ProjectionDomain::World);
        {
            DrawContextScope hud(rsf::anomaly::temporal::ViewId::Scope, rsf::anomaly::temporal::ProjectionDomain::FirstPerson);
            DrawScope hands(identities, contextual_key, DrawKind::Rigid, world);
            CHECK(borrow_draw()->domain == rsf::anomaly::temporal::ProjectionDomain::FirstPerson);
            std::thread worker([] { CHECK(!borrow_draw() && !capture_current_draw()); });
            worker.join();
        }
        CHECK(borrow_draw()->domain == rsf::anomaly::temporal::ProjectionDomain::World);
    }
    {
        DrawScope no_world(identities, contextual_key, DrawKind::Rigid, nullptr);
        CHECK(capture_current_draw() && !capture_current_draw()->world);
        CHECK(!borrow_draw()->view && !borrow_draw()->domain);
    }
    world[0] = std::numeric_limits<float>::quiet_NaN();
    { DrawScope invalid(identities, contextual_key, DrawKind::Rigid, world); CHECK(!capture_current_draw()->world); }
    world[0] = 1;
    std::array<std::byte, 256> parent{};
    std::array<std::byte, 64> skeleton{};
    std::array<std::byte, 336> bones{};
    const auto store = [](auto& buffer, std::size_t offset, const auto& value) { std::memcpy(buffer.data() + offset, &value, sizeof(value)); };
    const auto parent_pointer = parent.data();
    const auto bone_pointer = bones.data();
    store(skeleton, 8, parent_pointer);
    store(skeleton, 56, std::uint16_t{2});
    store(skeleton, 60, std::uint32_t{2});
    store(parent, 208, bone_pointer);
    std::memcpy(bones.data() + 64, world, sizeof(world));
    world[12] = 37;
    std::memcpy(bones.data() + 168 + 64, world, sizeof(world));
    {
        DrawScope skin(identities, contextual_key, DrawKind::HardwareSkinned, world, skeleton.data());
        const auto copy = capture_current_draw();
        CHECK(copy && copy->bones.size() == 2 && copy->palette_layout != 0);
        CHECK(copy->bones[0].at(0, 3) == 17 && copy->bones[1].at(0, 3) == 37);
        store(skeleton, 60, std::uint32_t{1025});
        CHECK(capture_current_draw()->bones.empty() && capture_current_draw()->palette_layout == 0);
        store(skeleton, 56, std::uint16_t{0});
        CHECK(capture_current_draw()->bones.empty());
    }

    // Feed into the real temporal history types.
    namespace temporal = rsf::anomaly::temporal;
    const auto make_frame = [](temporal::ViewId view, std::uint64_t source) {
        temporal::FrameInput frame;
        frame.view = view;
        frame.source_frame = source;
        frame.world = {temporal::Mat4::identity(), temporal::Mat4::identity(), {}};
        frame.resolutions = *temporal::plan_resolutions({1920, 1080}, {1280, 720}, {1024, 512});
        return frame;
    };
    float feed_world[16]{1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 3, 4, 5, 1};
    IdentityRegistry feed_identities;
    const auto rigid_key = feed_identities.acquire(0x1, 0x100);
    const auto skin_key = feed_identities.acquire(0x1, 0x200);
    CHECK(rigid_key && skin_key);
    temporal::TemporalHistory history;
    RetirementQueue queue;
    const auto first = history.begin_frame(make_frame(temporal::ViewId::Main, 10));
    CHECK(first.status == temporal::BeginStatus::Started && first.token);
    CHECK(feed_current_draw(history, *first.token, queue).status == FeedStatus::NoDraw);
    {
        DrawScope no_context(feed_identities, rigid_key, DrawKind::Rigid, feed_world);
        CHECK(feed_current_draw(history, *first.token, queue).status == FeedStatus::MissingContext);
    }
    {
        DrawContextScope view(temporal::ViewId::Scope, temporal::ProjectionDomain::World);
        DrawScope draw(feed_identities, rigid_key, DrawKind::Rigid, feed_world);
        CHECK(feed_current_draw(history, *first.token, queue).status == FeedStatus::WrongView);
    }
    {
        DrawContextScope view(temporal::ViewId::Main, temporal::ProjectionDomain::World);
        DrawScope draw(feed_identities, rigid_key, DrawKind::Rigid, feed_world);
        const auto fed = feed_current_draw(history, *first.token, queue);
        CHECK(fed.status == FeedStatus::Captured && fed.history == temporal::CaptureStatus::Captured);
        CHECK(feed_current_draw(history, *first.token, queue).history == temporal::CaptureStatus::Repeated);
        feed_world[12] = 99; // A different world inside the same frame must conflict, never overwrite.
        const auto conflict = feed_current_draw(history, *first.token, queue);
        CHECK(conflict.status == FeedStatus::Rejected && conflict.history == temporal::CaptureStatus::Conflict);
        feed_world[12] = 3;
        // The first-person domain has no projection in this frame, so history refuses it.
        DrawContextScope hands(temporal::ViewId::Main, temporal::ProjectionDomain::FirstPerson);
        DrawScope held(feed_identities, rigid_key, DrawKind::Rigid, feed_world);
        const auto refused = feed_current_draw(history, *first.token, queue);
        CHECK(refused.status == FeedStatus::Rejected && refused.history == temporal::CaptureStatus::MissingProjection);
    }
    for (const auto kind : {DrawKind::SingleBone, DrawKind::SoftwareSkinned, DrawKind::Tree}) {
        DrawContextScope view(temporal::ViewId::Main, temporal::ProjectionDomain::World);
        DrawScope draw(feed_identities, rigid_key, kind, feed_world);
        CHECK(feed_current_draw(history, *first.token, queue).status == FeedStatus::UnsupportedKind);
    }
    {
        DrawContextScope view(temporal::ViewId::Main, temporal::ProjectionDomain::World);
        DrawScope draw(feed_identities, rigid_key, DrawKind::Rigid, nullptr);
        CHECK(feed_current_draw(history, *first.token, queue).status == FeedStatus::MissingWorld);
    }
    // Hardware skinning, using the layout buffers built above (two bones at +64 with stride 168).
    store(skeleton, 56, std::uint16_t{2});
    store(skeleton, 60, std::uint32_t{2});
    {
        DrawContextScope view(temporal::ViewId::Main, temporal::ProjectionDomain::World);
        DrawScope draw(feed_identities, skin_key, DrawKind::HardwareSkinned, feed_world, skeleton.data());
        const auto fed = feed_current_draw(history, *first.token, queue);
        CHECK(fed.status == FeedStatus::Captured && fed.history == temporal::CaptureStatus::Captured);
        store(skeleton, 60, std::uint32_t{0}); // Unreadable palette must not become an invented identity pose.
        CHECK(feed_current_draw(history, *first.token, queue).status == FeedStatus::MissingPalette);
        store(skeleton, 60, std::uint32_t{2});
    }
    // The next source frame links the previous snapshot by identity; a recycled generation does not.
    const auto second = history.begin_frame(make_frame(temporal::ViewId::Main, 11));
    CHECK(second.status == temporal::BeginStatus::Started && second.token);
    temporal::CaptureResult linked;
    {
        DrawContextScope view(temporal::ViewId::Main, temporal::ProjectionDomain::World);
        DrawScope draw(feed_identities, rigid_key, DrawKind::Rigid, feed_world);
        const auto capture = capture_current_draw();
        FeedStatus why{};
        const auto snapshot = to_snapshot(*capture, why);
        CHECK(snapshot && why == FeedStatus::Captured);
        linked = history.capture(*second.token, capture->key, *snapshot);
        CHECK(linked.status == temporal::CaptureStatus::Captured && linked.previous && linked.previous->world.at(0, 3) == 3);
    }
    // Retirement crosses threads through the queue and is applied by the history owner.
    std::thread lifecycle([&] { RetirementQueue::sink(&queue, *rigid_key); RetirementQueue::sink(nullptr, *rigid_key); });
    lifecycle.join();
    feed_identities.retire(rigid_key->owner, rigid_key->visual);
    CHECK(queue.dropped() == 0);
    CHECK(queue.drain(history) == 1 && queue.drain(history) == 0);
    temporal::InstanceSnapshot retired_snapshot;
    retired_snapshot.world = temporal::Mat4::identity();
    CHECK(history.capture(*second.token, *rigid_key, retired_snapshot).status == temporal::CaptureStatus::Retired);
    const auto recycled_key = feed_identities.acquire(0x1, 0x100);
    CHECK(recycled_key && recycled_key->generation > rigid_key->generation);
    {
        DrawContextScope view(temporal::ViewId::Main, temporal::ProjectionDomain::World);
        DrawScope draw(feed_identities, recycled_key, DrawKind::Rigid, feed_world);
        const auto fed = feed_current_draw(history, *second.token, queue);
        CHECK(fed.status == FeedStatus::Captured && fed.history == temporal::CaptureStatus::Captured);
    }
    CHECK(geometry_stats().capture_faults == 0);
    std::puts("Anomaly geometry registry, TLS, real-layout world/palette capture, history feed: PASS");
}
