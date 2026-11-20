// SPDX-License-Identifier: MIT
#include "motion_policy.h"

#include <type_traits>

namespace rsf::anomaly::motion {
namespace {
struct SnapshotInput {
    DrawSnapshotView draw{};
    std::uint64_t palette_layout{};
    const temporal::Vec4* deformation_constants{};
    std::size_t deformation_count{};
    std::uint64_t deformation_layout{};
    const double* detail{};
    temporal::ProjectionDomain domain{};
    temporal::InstanceKind kind{};
    bool has_deformation{};
};

static_assert(std::is_standard_layout_v<temporal::Mat4> && sizeof(temporal::Mat4) == 128);
static_assert(sizeof(temporal::Vec4) == 32 && sizeof(temporal::InstanceKey) == 24);
static_assert(sizeof(CameraSample) == 256 && offsetof(CameraSample, projection) == 128);
static_assert(sizeof(PaletteView) == 16 && sizeof(DrawSnapshotView) == 24);
static_assert(sizeof(SnapshotInput) == 72 && offsetof(SnapshotInput, domain) == 64);
static_assert(sizeof(CoreDrawInput) == 48 && offsetof(CoreDrawInput, previous) == 24);
static_assert(sizeof(CoreDecision) == 408 && offsetof(CoreDecision, bones) == 392);
static_assert(sizeof(TreeRecord) == 104 && offsetof(TreeRecord, lod) == 96);
static_assert(sizeof(TreeDrawInput) == 144 && offsetof(TreeDrawInput, current) == 24);
static_assert(sizeof(DetailDrawInput) == 32 && offsetof(DetailDrawInput, live_groups) == 20);
static_assert(sizeof(TreeDecision) == 136 && sizeof(DetailDecision) == 136);
static_assert(sizeof(FoliageConstants) == 48 && sizeof(FoliageItem) == 76 && offsetof(FoliageItem, rows) == 12);
static_assert(sizeof(PixelInputs) == 32 && sizeof(CoreHistory) == 3);
static_assert(static_cast<unsigned>(temporal::ViewId::Main) == 0 && static_cast<unsigned>(temporal::ViewId::Scope) == 1);
static_assert(static_cast<unsigned>(temporal::ProjectionDomain::World) == 0 &&
    static_cast<unsigned>(temporal::ProjectionDomain::FirstPerson) == 1);
static_assert(static_cast<unsigned>(Family::CoreStatic) == 1 && static_cast<unsigned>(Family::CoreModel) == 2 &&
    static_cast<unsigned>(Family::FoliageTree) == 4 && static_cast<unsigned>(Family::FoliageDetail) == 5);
using geometry::DrawKind;
using geometry::DrawVerdict;
static_assert(static_cast<unsigned>(DrawKind::Rigid) == 0 && static_cast<unsigned>(DrawKind::HardwareSkinned) == 1 &&
    static_cast<unsigned>(DrawKind::SingleBone) == 2 && static_cast<unsigned>(DrawKind::Tree) == 4);
static_assert(static_cast<unsigned>(DrawVerdict::Identified) == 0 &&
    static_cast<unsigned>(DrawVerdict::IgnoredStaticWorld) == 2 && static_cast<unsigned>(DrawVerdict::IdentifiedFoliage) == 8);
}

// Private static-library ABI. All storage is borrowed for the call except the opaque history owner and its draw lease.
extern "C" {
MotionHistoryState* rsf_anomaly_motion_history_create() noexcept;
void rsf_anomaly_motion_history_destroy(MotionHistoryState*) noexcept;
bool rsf_anomaly_motion_begin(MotionHistoryState*, temporal::ViewId, std::uint64_t, std::uint64_t, std::uint32_t, std::uint32_t) noexcept;
bool rsf_anomaly_motion_active(const MotionHistoryState*, temporal::ViewId) noexcept;
CameraStatus rsf_anomaly_motion_observe(MotionHistoryState*, temporal::ViewId, temporal::ProjectionDomain,
    const CameraSample*) noexcept;
const CameraSample* rsf_anomaly_motion_camera(const MotionHistoryState*, temporal::ViewId, temporal::ProjectionDomain) noexcept;
void rsf_anomaly_motion_record(MotionHistoryState*, temporal::ViewId, const temporal::InstanceKey*,
    const SnapshotInput*, DrawSnapshotView*) noexcept;
void rsf_anomaly_motion_retire(MotionHistoryState*, const temporal::InstanceKey*) noexcept;
void rsf_anomaly_motion_reset(MotionHistoryState*) noexcept;
std::uint64_t rsf_anomaly_motion_conflicts(const MotionHistoryState*) noexcept;

bool rsf_anomaly_motion_pack_core(const temporal::Mat4*, const temporal::Mat4*, std::size_t, const CoreHistory*, std::byte*) noexcept;
void rsf_anomaly_motion_pack_core_invalid(std::byte*) noexcept;
bool rsf_anomaly_motion_pack_tess(const temporal::Mat4*, const temporal::Mat4*, bool, std::byte*) noexcept;
void rsf_anomaly_motion_pack_tess_invalid(std::byte*) noexcept;
void rsf_anomaly_motion_pack_foliage_invalid(std::byte*) noexcept;
bool rsf_anomaly_motion_pack_tree(const temporal::Mat4*, const float*, const FoliageConstants*, std::byte*) noexcept;
bool rsf_anomaly_motion_pack_detail(const temporal::Mat4*, const FoliageConstants*, const FoliageItem*, std::size_t,
    std::uint32_t, std::byte*) noexcept;
bool rsf_anomaly_motion_pack_pixel(const PixelInputs*, std::byte*) noexcept;
void rsf_anomaly_motion_remove_jitter(const temporal::Mat4*, float, float, temporal::Mat4*) noexcept;
void rsf_anomaly_motion_from_engine(const float*, temporal::Mat4*) noexcept;
void rsf_anomaly_motion_compose(const temporal::Mat4*, const temporal::Mat4*, temporal::Mat4*) noexcept;

const char* rsf_anomaly_motion_history_reason_name(HistoryReason) noexcept;
void rsf_anomaly_motion_decide_core(const CoreDrawInput*, CoreDecision*) noexcept;
void rsf_anomaly_motion_decide_tree(const TreeDrawInput*, TreeDecision*) noexcept;
void rsf_anomaly_motion_decide_detail(const DetailDrawInput*, DetailDecision*) noexcept;
std::size_t rsf_anomaly_motion_distance_band(double) noexcept;
const char* rsf_anomaly_motion_distance_band_name(std::size_t) noexcept;
double rsf_anomaly_motion_camera_distance(const temporal::Mat4*, const temporal::Mat4*) noexcept;
ModelElement rsf_anomaly_motion_model_element(const char*, std::size_t) noexcept;
bool rsf_anomaly_motion_stale_palette(const DrawSnapshotView*, const DrawSnapshotView*, double) noexcept;
}

MotionHistory::~MotionHistory() { rsf_anomaly_motion_history_destroy(state_); }

bool MotionHistory::begin_frame(temporal::ViewId view, std::uint64_t frame, std::uint64_t generation,
    std::uint32_t width, std::uint32_t height)
{
    if (!state_) state_ = rsf_anomaly_motion_history_create();
    return rsf_anomaly_motion_begin(state_, view, frame, generation, width, height);
}

bool MotionHistory::active(temporal::ViewId view) const noexcept { return rsf_anomaly_motion_active(state_, view); }

CameraStatus MotionHistory::observe_camera(temporal::ViewId view, temporal::ProjectionDomain domain,
    const CameraSample& camera)
{
    return rsf_anomaly_motion_observe(state_, view, domain, &camera);
}

const CameraSample* MotionHistory::previous_camera(temporal::ViewId view,
    temporal::ProjectionDomain domain) const noexcept
{
    return rsf_anomaly_motion_camera(state_, view, domain);
}

DrawSnapshotView MotionHistory::record(temporal::ViewId view, temporal::InstanceKey key,
    const temporal::InstanceSnapshot& snapshot)
{
    SnapshotInput in;
    in.draw = draw_snapshot_view(snapshot);
    in.palette_layout = snapshot.palette_layout;
    in.domain = snapshot.domain;
    in.kind = snapshot.kind;
    if (snapshot.deformation) {
        in.has_deformation = true;
        in.deformation_layout = snapshot.deformation->layout;
        in.deformation_constants = snapshot.deformation->constants.data();
        in.deformation_count = snapshot.deformation->constants.size();
    }
    if (snapshot.detail) in.detail = &snapshot.detail->fade_scale;
    DrawSnapshotView previous;
    rsf_anomaly_motion_record(state_, view, &key, &in, &previous);
    return previous;
}

void MotionHistory::retire(temporal::InstanceKey key)
{
    if (!state_) state_ = rsf_anomaly_motion_history_create();
    rsf_anomaly_motion_retire(state_, &key);
}
void MotionHistory::reset_all() { rsf_anomaly_motion_reset(state_); }
std::uint64_t MotionHistory::conflicts() const noexcept { return rsf_anomaly_motion_conflicts(state_); }

bool pack_core_vertex(const temporal::Mat4& previous_wvp, std::span<const temporal::Mat4> bones, CoreHistory history,
    CoreVertexBytes& out) noexcept
{
    return rsf_anomaly_motion_pack_core(&previous_wvp, bones.data(), bones.size(), &history, out.data());
}
void pack_core_invalid(CoreVertexBytes& out) noexcept { rsf_anomaly_motion_pack_core_invalid(out.data()); }
void pack_tess_invalid(TessDomainBytes& out) noexcept { rsf_anomaly_motion_pack_tess_invalid(out.data()); }
bool pack_tess_domain(const temporal::Mat4& previous_wv, const temporal::Mat4& previous_projection, bool valid,
    TessDomainBytes& out) noexcept
{
    return rsf_anomaly_motion_pack_tess(&previous_wv, &previous_projection, valid, out.data());
}
void pack_foliage_invalid(FoliageVertexBytes& out) noexcept { rsf_anomaly_motion_pack_foliage_invalid(out.data()); }
bool pack_foliage_tree(const temporal::Mat4& previous_clip, const float (&xform)[12], const FoliageConstants& previous,
    FoliageVertexBytes& out) noexcept
{
    return rsf_anomaly_motion_pack_tree(&previous_clip, xform, &previous, out.data());
}
bool pack_foliage_detail(const temporal::Mat4& previous_clip, const FoliageConstants& previous,
    std::span<const FoliageItem> items, std::uint32_t groups, FoliageVertexBytes& out) noexcept
{
    return rsf_anomaly_motion_pack_detail(&previous_clip, &previous, items.data(), items.size(), groups, out.data());
}
bool pack_pixel(const PixelInputs& in, PixelBytes& out) noexcept { return rsf_anomaly_motion_pack_pixel(&in, out.data()); }

temporal::Mat4 remove_jitter(const temporal::Mat4& projection, float jitter_u, float jitter_v) noexcept
{
    temporal::Mat4 out;
    rsf_anomaly_motion_remove_jitter(&projection, jitter_u, jitter_v, &out);
    return out;
}
temporal::Mat4 from_engine(const float (&values)[16]) noexcept
{
    temporal::Mat4 out;
    rsf_anomaly_motion_from_engine(values, &out);
    return out;
}
temporal::Mat4 compose(const temporal::Mat4& a, const temporal::Mat4& b) noexcept
{
    temporal::Mat4 out;
    rsf_anomaly_motion_compose(&a, &b, &out);
    return out;
}

const char* history_reason_name(HistoryReason reason) noexcept { return rsf_anomaly_motion_history_reason_name(reason); }
CoreDecision decide_core(const CoreDrawInput& in) noexcept
{
    CoreDecision out;
    rsf_anomaly_motion_decide_core(&in, &out);
    return out;
}
TreeDecision decide_tree(const TreeDrawInput& in) noexcept
{
    TreeDecision out;
    rsf_anomaly_motion_decide_tree(&in, &out);
    return out;
}
DetailDecision decide_detail(const DetailDrawInput& in) noexcept
{
    DetailDecision out;
    rsf_anomaly_motion_decide_detail(&in, &out);
    return out;
}
std::size_t distance_band(double metres) noexcept { return rsf_anomaly_motion_distance_band(metres); }
const char* distance_band_name(std::size_t band) noexcept { return rsf_anomaly_motion_distance_band_name(band); }
double camera_distance(const temporal::Mat4& view, const temporal::Mat4& world) noexcept
{
    return rsf_anomaly_motion_camera_distance(&view, &world);
}
ModelElement model_element(std::string_view root) noexcept
{
    return rsf_anomaly_motion_model_element(root.data(), root.size());
}
bool stale_palette(DrawSnapshotView previous, DrawSnapshotView current, double min_move) noexcept
{
    return rsf_anomaly_motion_stale_palette(&previous, &current, min_move);
}
}
