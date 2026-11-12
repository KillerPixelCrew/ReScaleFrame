#include "games/anomaly/temporal/planning.h"
#include "games/anomaly/temporal/math.h"
#include "games/anomaly/temporal/history.h"
#include "games/anomaly/temporal/frame_plan.h"
#include <cmath>
#include <limits>

#include <cstdlib>
#include <iostream>
#include <stdexcept>
#include <string_view>
#include <vector>

using namespace rsf::anomaly::temporal;

struct TestCase { const char* name; void (*run)(); };
std::vector<TestCase>& cases() { static std::vector<TestCase> value; return value; }
struct RegisterTest {
    RegisterTest(const char* name, void (*run)()) { cases().push_back({name, run}); }
};
#define TEST(name) void name(); const RegisterTest register_##name{#name, name}; void name()

void require(bool condition, const char* message)
{
    if (!condition) {
        throw std::runtime_error(message);
    }
}

TEST(independent_dimensions)
{
    const auto plan = plan_resolutions({1920, 1080}, {1280, 720}, {1024, 512});
    require(plan.has_value(), "independent dimensions must be accepted");
    require(plan->output == Extent{1920, 1080}, "output dimensions changed");
    require(plan->render == Extent{1280, 720}, "render dimensions changed");
    require(plan->pda == Extent{1024, 512}, "PDA dimensions were conflated with world");
}

TEST(dimension_validation)
{
    require(!plan_resolutions({0, 1080}, {640, 360}, {512, 512}), "zero output accepted");
    require(!plan_resolutions({1920, 1080}, {1, 360}, {512, 512}), "subminimum render accepted");
    require(!plan_resolutions({1920, 1080}, {640, 360}, {512, 1}), "subminimum PDA accepted");
    require(!plan_resolutions({1920, 1080}, {2048, 360}, {512, 512}), "SR render exceeded output");
    require(!plan_resolutions({20000, 1080}, {640, 360}, {512, 512}), "oversize texture accepted");
    require(plan_resolutions({2, 2}, {2, 2}, {2, 2}).has_value(), "minimum valid rejected");
}

TEST(matrix_motion)
{
    auto world = Mat4::identity();
    world.at(0, 3) = 0.2;
    const auto mvp = multiply(Mat4::identity(), world);
    require(mvp.has_value(), "matrix multiplication rejected");
    const auto current = transform(*mvp, {0, 0, 0.5, 1});
    const auto previous = transform(Mat4::identity(), {0, 0, 0.5, 1});
    require(current && previous, "valid homogeneous transform rejected");
    const auto motion = pixel_motion(*current, *previous, {100, 50});
    require(motion && std::abs(motion->x + 10) < 1e-9 && motion->y == 0,
        "current-to-previous motion must be -10 pixels for rightward geometry");
    const auto y_motion = pixel_motion({0, 0.4, 0.5, 1}, {0, 0, 0.5, 1}, {100, 50});
    require(y_motion && std::abs(y_motion->y - 10) < 1e-9, "top-left UV Y convention wrong");
    const auto projection = jitter_projection(Mat4::identity(), {0.25, -0.5}, {100, 50});
    require(projection.has_value(), "valid jitter rejected");
    const auto clip = transform(*projection, {0, 0, 0.5, 1});
    require(clip.has_value(), "jitter projection failed");
    const auto stable = pixel_motion_from_jittered(*clip, {0, 0, 0.5, 1}, {100, 50}, {0.25, -0.5}, {});
    require(stable && std::abs(stable->x) < 1e-9 && std::abs(stable->y) < 1e-9,
        "jitter-only displacement leaked into geometric motion");
}

CameraState camera_fixture()
{
    return {Mat4::identity(), Mat4::identity(), {}};
}

InstanceSnapshot rigid_fixture()
{
    InstanceSnapshot pose;
    pose.world = Mat4::identity();
    return pose;
}

FrameInput frame(ViewId view, std::uint64_t source)
{
    FrameInput input;
    input.view = view;
    input.source_frame = source;
    input.world = camera_fixture();
    input.resolutions = *plan_resolutions({1920, 1080}, {1280, 720}, {1024, 512});
    return input;
}

TEST(view_timelines_do_not_follow_present)
{
    TemporalHistory history;
    const auto main1 = history.begin_frame(frame(ViewId::Main, 100));
    require(main1.status == BeginStatus::Started && main1.token.has_value(), "first main frame failed");
    require(!history.cameras(*main1.token, ProjectionDomain::World)->previous,
        "first frame invented camera history");
    const auto scope1 = history.begin_frame(frame(ViewId::Scope, 101));
    require(scope1.token.has_value(), "scope must advance without Present");
    auto main_input = frame(ViewId::Main, 102);
    main_input.world.view.at(0, 3) = 2;
    const auto main2 = history.begin_frame(main_input);
    require(main2.token.has_value(), "second main frame failed");
    const auto main_pair = history.cameras(*main2.token, ProjectionDomain::World);
    require(main_pair && main_pair->previous && main_pair->previous_source_frame == 100,
        "main used immediately preceding scope frame as history");
    require(main_pair->previous->view.at(0, 3) == 0, "previous camera overwritten");
    const auto repeat = history.begin_frame(main_input);
    require(repeat.status == BeginStatus::Repeated && repeat.token == main2.token,
        "same-frame pass advanced history");
    require(history.cameras(*scope1.token, ProjectionDomain::World).has_value(),
        "main frame invalidated independent scope token");
    const auto scope2 = history.begin_frame(frame(ViewId::Scope, 103));
    require(history.cameras(*scope2.token, ProjectionDomain::World)->previous_source_frame == 101,
        "scope history followed main instead of scope");
    require(!history.cameras(*main1.token, ProjectionDomain::World), "stale main frame token accepted");
}

TEST(reject_frame_mutation_and_bad_inputs)
{
    TemporalHistory history;
    const auto input = frame(ViewId::Main, 20);
    const auto start = history.begin_frame(input);
    auto changed = input;
    changed.world.view.at(0, 3) = 1;
    require(history.begin_frame(changed).status == BeginStatus::Conflict, "same-frame camera mutation accepted");
    require(history.begin_frame(frame(ViewId::Main, 19)).status == BeginStatus::OutOfOrder,
        "out-of-order source frame accepted");
    auto invalid = frame(ViewId::Main, 21);
    invalid.world.projection.at(0, 0) = std::numeric_limits<double>::quiet_NaN();
    require(history.begin_frame(invalid).status == BeginStatus::Invalid, "NaN camera accepted");
    invalid = frame(ViewId::Main, 21);
    invalid.resolutions.render.width = 0;
    require(history.begin_frame(invalid).status == BeginStatus::Invalid, "invalid dimensions accepted at frame boundary");
    require(history.cameras(*start.token, ProjectionDomain::World)->current == input.world,
        "rejected input poisoned current frame");
    invalid = frame(ViewId::Main, 21);
    invalid.view = static_cast<ViewId>(99);
    require(history.begin_frame(invalid).status == BeginStatus::Invalid, "invalid view accepted");
    require(!history.cameras({static_cast<ViewId>(99), 20, 1}, ProjectionDomain::World),
        "invalid token view not rejected");
    require(!history.cameras(*start.token, static_cast<ProjectionDomain>(99)), "invalid domain accepted");
}

TEST(camera_resets_and_first_person_are_separate)
{
    TemporalHistory history;
    auto input = frame(ViewId::Main, 1);
    input.first_person = camera_fixture();
    input.first_person->projection.at(0, 0) = 3;
    history.begin_frame(input);
    input.source_frame = 3;
    input.first_person->projection.at(0, 0) = 4;
    auto next = history.begin_frame(input);
    const auto fp = history.cameras(*next.token, ProjectionDomain::FirstPerson);
    require(fp && fp->previous && fp->previous->projection.at(0, 0) == 3 &&
        fp->current.projection.at(0, 0) == 4, "first-person projections not retained independently");
    input.source_frame = 5;
    input.first_person_cut = true;
    next = history.begin_frame(input);
    require(!history.cameras(*next.token, ProjectionDomain::FirstPerson)->previous,
        "first-person cut reused history");
    require(history.cameras(*next.token, ProjectionDomain::World)->previous.has_value(),
        "first-person cut invalidated main world history");
    input.source_frame = 7;
    input.first_person.reset();
    input.first_person_cut = false;
    next = history.begin_frame(input);
    require(!history.cameras(*next.token, ProjectionDomain::FirstPerson), "missing first-person fell back to world");
    input.source_frame = 9;
    input.camera_cut = true;
    next = history.begin_frame(input);
    require(!history.cameras(*next.token, ProjectionDomain::World)->previous, "camera cut did not reset world");
    input.source_frame = 11;
    input.camera_cut = false;
    input.resolutions.render = {960, 540};
    next = history.begin_frame(input);
    require(!history.cameras(*next.token, ProjectionDomain::World)->previous, "render resize reused history");
    input.source_frame = 13;
    next = history.begin_frame(input);
    require(history.cameras(*next.token, ProjectionDomain::World)->previous.has_value(),
        "history did not recover after resize");
    input.source_frame = 15;
    input.resolutions.output = {2560, 1440};
    next = history.begin_frame(input);
    require(!history.cameras(*next.token, ProjectionDomain::World)->previous, "output resize reused history");
}

TEST(rigid_instances_freeze_within_source_frame)
{
    TemporalHistory history;
    const InstanceKey a{10, 100, 1};
    const InstanceKey b{11, 100, 1};
    auto pose = rigid_fixture();
    auto start = history.begin_frame(frame(ViewId::Main, 10));
    const auto first = history.capture(*start.token, a, pose);
    require(first.status == CaptureStatus::Captured && first.current && !first.previous,
        "first-seen rigid instance invented previous pose");
    pose.world.at(0, 3) = 7;
    require(first.current->world.at(0, 3) == 0, "snapshot aliases producer world matrix");
    auto next = history.begin_frame(frame(ViewId::Main, 12));
    const auto current = history.capture(*next.token, a, pose);
    require(current.previous && current.previous->world.at(0, 3) == 0 && current.current->world.at(0, 3) == 7,
        "previous rigid world matrix lost");
    const auto repeat = history.capture(*next.token, a, pose);
    require(repeat.status == CaptureStatus::Repeated && repeat.current == current.current &&
        repeat.previous == current.previous, "multipass capture advanced or recopied frozen pose");
    pose.world.at(0, 3) = 8;
    require(history.capture(*next.token, a, pose).status == CaptureStatus::Conflict,
        "same-frame conflicting rigid transform accepted");
    require(current.current->world.at(0, 3) == 7, "conflict mutated frozen current");
    const auto other = history.capture(*next.token, b, pose);
    require(!other.previous, "new object reused another object's visual history");
    require(history.capture(*start.token, a, pose).status == CaptureStatus::StaleFrame,
        "stale producer token mutated history");
}

TEST(lifetime_retirement_missing_visibility_and_view_isolation)
{
    TemporalHistory history;
    const InstanceKey key{42, 500, 3};
    auto pose = rigid_fixture();
    auto main = history.begin_frame(frame(ViewId::Main, 1));
    history.capture(*main.token, key, pose);
    auto scope = history.begin_frame(frame(ViewId::Scope, 2));
    require(!history.capture(*scope.token, key, pose).previous, "scope reused main object pose");
    main = history.begin_frame(frame(ViewId::Main, 3));
    require(history.capture(*main.token, key, pose).previous != nullptr, "visible main pose lost");
    require(!history.capture(*main.token, {42, 501, 3}, pose).previous, "visual replacement reused history");
    require(!history.capture(*main.token, {42, 500, 4}, pose).previous, "new generation reused history");
    history.begin_frame(frame(ViewId::Main, 5)); // key was not drawn in this source frame.
    main = history.begin_frame(frame(ViewId::Main, 7));
    require(!history.capture(*main.token, key, pose).previous, "newly visible object reused stale history");
    require(history.retire(key), "valid lifetime retirement failed");
    require(history.capture(*main.token, key, pose).status == CaptureStatus::Retired,
        "retired main object still accepted");
    require(history.capture(*scope.token, key, pose).status == CaptureStatus::Retired,
        "retirement did not invalidate scope");
    require(history.capture(*main.token, {42, 500, 2}, pose).status == CaptureStatus::Retired,
        "older generation resurrected after retirement");
    const auto replacement = history.capture(*main.token, {42, 500, 4}, pose);
    require(replacement.status == CaptureStatus::Captured && !replacement.previous,
        "replacement inherited retired pose");
}

TEST(bone_palettes_are_owned_and_multipass_frozen)
{
    TemporalHistory history;
    const InstanceKey key{5, 6, 1};
    auto pose = rigid_fixture();
    pose.kind = InstanceKind::Skinned;
    pose.palette_layout = 77;
    pose.bones = {Mat4::identity(), Mat4::identity()};
    pose.bones[1].at(1, 3) = 4;
    auto f = history.begin_frame(frame(ViewId::Main, 1));
    const auto initial = history.capture(*f.token, key, pose);
    pose.bones[1].at(1, 3) = 9;
    require(initial.current->bones[1].at(1, 3) == 4, "bone snapshot aliases current-only producer palette");
    f = history.begin_frame(frame(ViewId::Main, 3));
    const auto next = history.capture(*f.token, key, pose);
    require(next.previous && next.previous->bones[1].at(1, 3) == 4, "previous bone snapshot lost");
    const auto again = history.capture(*f.token, key, pose);
    require(again.previous == next.previous && again.current == next.current, "shadow/main passes advanced bones twice");
    pose.bones[0].at(0, 3) = 12;
    require(history.capture(*f.token, key, pose).status == CaptureStatus::Conflict, "same-frame bone mutation accepted");
    f = history.begin_frame(frame(ViewId::Main, 5));
    pose.palette_layout = 78;
    require(!history.capture(*f.token, key, pose).previous, "different bone ordering reused palette history");
    f = history.begin_frame(frame(ViewId::Main, 7));
    pose.bones.pop_back();
    require(!history.capture(*f.token, key, pose).previous, "different bone count reused palette history");
}

TEST(detail_deformation_uses_item_lifetime_not_batch_order)
{
    TemporalHistory history;
    const InstanceKey item_a{1001, 90, 1};
    const InstanceKey item_b{1002, 90, 1};
    auto a = rigid_fixture();
    a.kind = InstanceKind::Detail;
    a.deformation = DeformationState{99, {{1, 2, 3, 4}, {5, 6, 7, 8}}};
    a.detail = DetailState{0.25};
    auto b = a;
    b.world.at(0, 3) = 100;
    b.detail->fade_scale = 0.75;
    auto f = history.begin_frame(frame(ViewId::Main, 1));
    history.capture(*f.token, item_a, a);
    history.capture(*f.token, item_b, b);
    f = history.begin_frame(frame(ViewId::Main, 3));
    a.deformation->constants[0].w = 20;
    a.detail->fade_scale = 0.5;
    b.detail->fade_scale = 0.9;
    const auto current_b = history.capture(*f.token, item_b, b); // Repacked batch order.
    const auto current_a = history.capture(*f.token, item_a, a);
    require(current_a.previous && current_a.previous->detail->fade_scale == 0.25,
        "detail A reused another batch item's fade scale");
    require(current_b.previous && current_b.previous->detail->fade_scale == 0.75 &&
        current_b.previous->world.at(0, 3) == 100, "detail B identity did not survive repacking");
    require(current_a.previous->deformation->constants[0].w == 4 &&
        current_a.current->deformation->constants[0].w == 20, "previous deformation constants not owned");
    require(history.capture(*f.token, item_a, a).previous == current_a.previous,
        "detail multipass advanced deformation history");
    history.retire(item_a);
    require(!history.capture(*f.token, {1001, 90, 2}, a).previous, "recycled detail cache slot inherited history");
    f = history.begin_frame(frame(ViewId::Main, 5));
    b.deformation->layout = 100;
    require(!history.capture(*f.token, item_b, b).previous, "different deformation layout inherited history");
}

TEST(reject_incomplete_or_nonfinite_producer_state)
{
    TemporalHistory history;
    const auto f = history.begin_frame(frame(ViewId::Main, 1));
    auto pose = rigid_fixture();
    require(history.capture(*f.token, {0, 10, 1}, pose).status == CaptureStatus::Invalid,
        "zero owner identity accepted");
    require(history.capture(*f.token, {1, 0, 1}, pose).status == CaptureStatus::Invalid, "zero visual accepted");
    require(history.capture(*f.token, {1, 10, 0}, pose).status == CaptureStatus::Invalid, "zero generation accepted");
    require(!history.retire({0, 10, 1}), "invalid retirement accepted");
    std::uint64_t owner = 100;
    const auto rejected = [&](const InstanceSnapshot& sample) {
        const auto result = history.capture(*f.token, {++owner, 10, 1}, sample);
        require(result.status == CaptureStatus::Invalid && !result.current && !result.previous,
            "invalid producer snapshot accepted");
    };
    pose.world.at(0, 0) = std::numeric_limits<double>::infinity();
    rejected(pose);
    pose = rigid_fixture();
    pose.world.at(3, 0) = 1;
    rejected(pose);
    pose = rigid_fixture();
    pose.domain = static_cast<ProjectionDomain>(99);
    rejected(pose);
    pose = rigid_fixture();
    pose.kind = static_cast<InstanceKind>(99);
    rejected(pose);
    pose = rigid_fixture();
    pose.kind = InstanceKind::Skinned;
    rejected(pose); // Missing current palette cannot become camera-only motion.
    pose.bones = {Mat4::identity()};
    rejected(pose); // Missing palette layout identity.
    pose.palette_layout = 1;
    pose.bones[0].at(2, 3) = std::numeric_limits<double>::quiet_NaN();
    rejected(pose);
    pose.bones.assign(1025, Mat4::identity());
    rejected(pose);
    pose = rigid_fixture();
    pose.bones = {Mat4::identity()};
    rejected(pose); // A rigid classification must not silently ignore skinning.
    pose = rigid_fixture();
    pose.kind = InstanceKind::Deformed;
    rejected(pose);
    pose.deformation = DeformationState{1, {{0, 0, 0, 1}}};
    require(history.capture(*f.token, {++owner, 10, 1}, pose).status == CaptureStatus::Captured,
        "complete deformed producer state rejected");
    pose.deformation->constants[0].x = std::numeric_limits<double>::infinity();
    rejected(pose);
    pose.deformation = DeformationState{1, {{0, 0, 0, 1}}};
    pose.kind = InstanceKind::Detail;
    rejected(pose);
    pose.detail = DetailState{-0.01};
    rejected(pose);
    pose.detail->fade_scale = std::numeric_limits<double>::quiet_NaN();
    rejected(pose);
    pose.detail->fade_scale = 0;
    require(history.capture(*f.token, {++owner, 10, 1}, pose).status == CaptureStatus::Captured,
        "zero fade is valid item state, not absence of a producer");
    pose = rigid_fixture();
    pose.domain = ProjectionDomain::FirstPerson;
    const auto missing = history.capture(*f.token, {++owner, 10, 1}, pose);
    require(missing.status == CaptureStatus::MissingProjection && !missing.current && !missing.previous,
        "first-person snapshot used world projection fallback or hid the missing producer");
}

TEST(first_person_object_history_never_uses_world_projection)
{
    TemporalHistory history;
    const InstanceKey key{5, 6, 1};
    auto input = frame(ViewId::Main, 1);
    input.first_person = camera_fixture();
    input.first_person->projection.at(0, 0) = 3;
    auto world = rigid_fixture();
    auto hand = rigid_fixture();
    hand.domain = ProjectionDomain::FirstPerson;
    hand.world.at(0, 3) = 2;
    auto f = history.begin_frame(input);
    history.capture(*f.token, key, world);
    const auto first_hand = history.capture(*f.token, key, hand);
    require(first_hand.status == CaptureStatus::Captured,
        "world and HUD passes for the same object must have separate coordinate-space slots");
    input.source_frame = 3;
    f = history.begin_frame(input);
    world.world.at(0, 3) = 9;
    hand.world.at(0, 3) = 4;
    const auto next_world = history.capture(*f.token, key, world);
    const auto next_hand = history.capture(*f.token, key, hand);
    require(next_world.previous && next_world.previous->world.at(0, 3) == 0,
        "world draw inherited HUD-local transform");
    require(next_hand.previous && next_hand.previous->world.at(0, 3) == 2,
        "HUD draw inherited world transform");
    input.source_frame = 5;
    input.first_person_cut = true;
    f = history.begin_frame(input);
    require(!history.capture(*f.token, key, hand).previous, "HUD cut reused object history");
    require(history.capture(*f.token, key, world).previous != nullptr, "HUD cut reset world object history");
    input.source_frame = 7;
    input.first_person_cut = false;
    input.camera_cut = true;
    f = history.begin_frame(input);
    require(!history.capture(*f.token, key, hand).previous && !history.capture(*f.token, key, world).previous,
        "world cut did not reset both projection domains");
}

TEST(explicit_resets_invalidate_tokens_without_resurrecting_lifetimes)
{
    TemporalHistory history;
    TemporalHistory other;
    const InstanceKey key{4, 5, 1};
    const auto main = history.begin_frame(frame(ViewId::Main, 10));
    const auto foreign = other.begin_frame(frame(ViewId::Main, 10));
    require(!history.cameras(*foreign.token, ProjectionDomain::World), "foreign history token accepted");
    const auto scope = history.begin_frame(frame(ViewId::Scope, 11));
    history.capture(*main.token, key, rigid_fixture());
    history.retire(key);
    require(history.reset_view(ViewId::Main), "main reset rejected");
    require(!history.cameras(*main.token, ProjectionDomain::World), "reset did not invalidate old token");
    require(history.cameras(*scope.token, ProjectionDomain::World).has_value(), "main reset erased scope");
    const auto restarted = history.begin_frame(frame(ViewId::Main, 0));
    require(restarted.token && !history.cameras(*restarted.token, ProjectionDomain::World)->previous,
        "source counter reset reused old camera");
    require(history.capture(*restarted.token, key, rigid_fixture()).status == CaptureStatus::Retired,
        "device reset resurrected a retired object generation");
    require(!history.reset_view(static_cast<ViewId>(99)), "invalid reset view accepted");
    history.reset_all();
    require(!history.cameras(*scope.token, ProjectionDomain::World) &&
        !history.cameras(*restarted.token, ProjectionDomain::World), "all-view reset left live tokens");
}

TEST(inverse_and_projective_math_are_finite_and_ordered)
{
    auto view = Mat4::identity();
    view.at(0, 3) = -2;
    Mat4 projection;
    projection.at(0, 0) = 2;
    projection.at(1, 1) = 3;
    projection.at(2, 2) = 1.1;
    projection.at(2, 3) = -0.11;
    projection.at(3, 2) = 1;
    const auto vp = multiply(projection, view);
    require(vp.has_value(), "perspective view multiplication failed");
    const auto clip = transform(*vp, {2, 0, 1, 1});
    require(clip && clip->x == 0 && clip->w == 1, "matrix order or perspective W incorrect");
    const auto inv = inverse(*vp);
    require(inv.has_value(), "invertible camera rejected");
    const auto recovered = transform(*inv, *clip);
    require(recovered && std::abs(recovered->x - 2) < 1e-9 && std::abs(recovered->z - 1) < 1e-9,
        "inverse did not recover source homogeneous point");
    require(!inverse(Mat4{}), "singular matrix inverted");
    const auto jittered = jitter_projection(projection, {0.5, 0.25}, {100, 50});
    const auto jitter_clip = transform(*jittered, {0, 0, 0.5, 1});
    const auto plain_clip = transform(projection, {0, 0, 0.5, 1});
    const auto motion = pixel_motion_from_jittered(*jitter_clip, *plain_clip, {100, 50}, {0.5, 0.25}, {});
    require(motion && std::abs(motion->x) < 1e-9 && std::abs(motion->y) < 1e-9,
        "jitter must scale by clip W for perspective projection");
    auto nan = Mat4::identity();
    nan.at(0, 0) = std::numeric_limits<double>::quiet_NaN();
    require(!inverse(nan) && !multiply(nan, view) && !transform(nan, {0, 0, 1, 1}), "nonfinite matrices accepted");
    const double huge = std::numeric_limits<double>::max();
    auto large = Mat4::identity();
    large.at(0, 0) = huge;
    require(!multiply(large, large) && !transform(large, {2, 0, 1, 1}), "arithmetic overflow accepted");
    require(!pixel_motion({0, 0, 0, 0}, {0, 0, 0.5, 1}, {100, 50}), "zero W accepted");
    require(!pixel_motion({0, 0, 0.5, -1}, {0, 0, 0.5, 1}, {100, 50}), "behind-eye motion accepted");
    require(!pixel_motion({0, 0, 2, 1}, {0, 0, 0.5, 1}, {100, 50}), "outside depth clip accepted");
    require(!pixel_motion_from_jittered(*plain_clip, *plain_clip, {100, 50},
        {std::numeric_limits<double>::infinity(), 0}, {}), "infinite jitter accepted");
}

TEST(default_and_degenerate_camera_state_is_not_evidence)
{
    TemporalHistory history;
    auto input = frame(ViewId::Main, 1);
    input.world.projection = {};
    require(history.begin_frame(input).status == BeginStatus::Invalid, "singular projection accepted");
    input = frame(ViewId::Main, 1);
    input.world.view.at(2, 2) = 0;
    require(history.begin_frame(input).status == BeginStatus::Invalid, "singular view accepted");
    input = frame(ViewId::Main, 1);
    input.world.view.at(3, 0) = 1;
    require(history.begin_frame(input).status == BeginStatus::Invalid, "projective world-view transform accepted");
    FrameInput missing_camera;
    missing_camera.resolutions = frame(ViewId::Main, 1).resolutions;
    require(history.begin_frame(missing_camera).status == BeginStatus::Invalid, "default camera manufactured identity evidence");
    input = frame(ViewId::Main, 1);
    input.first_person = CameraState{};
    require(history.begin_frame(input).status == BeginStatus::Invalid, "default first-person camera manufactured evidence");
    input = frame(ViewId::Main, 1);
    input.world.jitter_pixels.x = std::numeric_limits<double>::max();
    require(history.begin_frame(input).status == BeginStatus::Invalid, "overflowing jitter projection accepted");
    input = frame(ViewId::Main, 1);
    const auto f = history.begin_frame(input);
    InstanceSnapshot missing_world;
    require(history.capture(*f.token, {1, 2, 1}, missing_world).status == CaptureStatus::Invalid,
        "default world matrix manufactured rigid producer evidence");
}

TEST(frame_plans_keep_geometry_output_and_pda_canvases_independent)
{
    TemporalHistory history;
    auto input = frame(ViewId::Main, 80);
    input.world.jitter_pixels = {0.25, 0.5};
    input.first_person = camera_fixture();
    input.first_person->projection.at(0, 0) = 3;
    input.first_person->jitter_pixels = {-0.25, -0.5};
    auto f = history.begin_frame(input);
    const auto plan = plan_frame_view(history, *f.token);
    require(plan && plan->frame == *f.token, "frame plan lost source/view identity");
    require(plan->world.viewport == Extent{1280, 720} && plan->reconstructed_world == Extent{1920, 1080},
        "world raster and reconstruction dimensions conflated");
    require(plan->screen_ui.viewport == Extent{1920, 1080} && plan->pda_content.viewport == Extent{1024, 512},
        "native screen UI and PDA content dimensions conflated");
    require(plan->world.projection == *jitter_projection(input.world.projection, {0.25, 0.5}, {1280, 720}),
        "world jitter used output dimensions");
    require(plan->first_person && plan->first_person->projection ==
        *jitter_projection(input.first_person->projection, {-0.25, -0.5}, {1280, 720}),
        "first-person pass reused main projection or main jitter");
    require(plan->presentation == PresentationRoute::RequiresHostDecision,
        "main plan asserted Present ownership without a verified host lease");
    const auto scope = history.begin_frame(frame(ViewId::Scope, 81));
    const auto scope_plan = plan_frame_view(history, *scope.token);
    require(scope_plan && scope_plan->presentation == PresentationRoute::OffscreenScope && !scope_plan->first_person,
        "scope planned as a presenting main view");
    f = history.begin_frame(frame(ViewId::Main, 82));
    require(!plan_frame_view(history, plan->frame), "stale frame produced a render plan");
}

TEST(matrix_indices_cannot_alias_other_rows)
{
    auto matrix = Mat4::identity();
    bool column_threw = false;
    try { matrix.at(0, 4) = 99; } catch (const std::out_of_range&) { column_threw = true; }
    require(column_threw && matrix.at(1, 0) == 0, "invalid column aliased a valid matrix row");
    const auto& immutable = matrix;
    bool row_threw = false;
    try { static_cast<void>(immutable.at(std::numeric_limits<std::size_t>::max(), 4)); }
    catch (const std::out_of_range&) { row_threw = true; }
    require(row_threw, "overflowed matrix index accepted");
}

int main(int argc, char** argv)
{
    try {
        std::size_t count = 0;
        for (const auto& test : cases()) {
            if (argc == 1 || std::string_view(argv[1]) == test.name) {
                test.run();
                ++count;
                std::cout << "PASS: " << test.name << '\n';
            }
        }
        require(count != 0, "unknown test selection");
        std::cout << count << " Anomaly temporal tests passed\n";
        return EXIT_SUCCESS;
    } catch (const std::exception& error) {
        std::cerr << "FAIL: " << error.what() << '\n';
        return EXIT_FAILURE;
    }
}
