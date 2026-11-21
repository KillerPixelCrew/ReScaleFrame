// SPDX-License-Identifier: MIT
#include "motion_foliage.h"
#include "shader_loader_platform.h"

namespace rsf::anomaly::motion {
static_assert(sizeof(TreeRecord) == 104 && offsetof(TreeRecord, lod) == 96);
static_assert(sizeof(DetailRemap) == 4704 && offsetof(DetailRemap, previous) == 20 && offsetof(DetailRemap, items) == 68);
static_assert(sizeof(DetailRows) == 3904);
static_assert(sizeof(FoliageLayout) == 108 && offsetof(FoliageLayout, xform) == 4 && offsetof(FoliageLayout, usable) == 104);

extern "C" {
FoliageHistoryState* rsf_anomaly_foliage_new() noexcept;
void rsf_anomaly_foliage_drop(FoliageHistoryState*) noexcept;
bool rsf_anomaly_foliage_begin(FoliageHistoryState*, temporal::ViewId, std::uint64_t, std::uint64_t, std::uint32_t, std::uint32_t) noexcept;
bool rsf_anomaly_foliage_active(const FoliageHistoryState*, temporal::ViewId) noexcept;
void rsf_anomaly_foliage_reset(FoliageHistoryState*) noexcept;
const TreeRecord* rsf_anomaly_foliage_tree(FoliageHistoryState*, temporal::ViewId, std::uint64_t, const TreeRecord*) noexcept;
void rsf_anomaly_foliage_detail(FoliageHistoryState*, temporal::ViewId, std::uint64_t, const FoliageConstants*, const DetailRows*, DetailRemap*) noexcept;
std::uint64_t rsf_anomaly_foliage_conflicts(const FoliageHistoryState*) noexcept;
std::uint64_t rsf_anomaly_foliage_overflows(const FoliageHistoryState*) noexcept;
std::size_t rsf_anomaly_foliage_recorded(const FoliageHistoryState*, temporal::ViewId) noexcept;
std::uint32_t rsf_anomaly_foliage_count(const DetailRows*) noexcept;
void rsf_anomaly_foliage_poison(DetailRows*) noexcept;
float rsf_anomaly_foliage_scale(const float*) noexcept;
bool rsf_anomaly_motion_foliage_layout(const rsf_anomaly_shader_reflection*, std::uint8_t, FoliageLayout*) noexcept;
}

FoliageLayout make_foliage_layout(Family family, const Reflection& stock) noexcept
{
    struct Call { Family family; FoliageLayout layout; } call{family, {}};
    call.layout.family = family;
    borrow_reflection(stock, &call, [](void* user, const rsf_anomaly_shader_reflection* view) {
        auto& c = *static_cast<Call*>(user);
        rsf_anomaly_motion_foliage_layout(view, static_cast<std::uint8_t>(c.family), &c.layout);
    });
    return call.layout;
}

std::uint32_t count_live_groups(const DetailRows& rows) noexcept { return rsf_anomaly_foliage_count(&rows); }
void poison_detail_rows(DetailRows& rows) noexcept { rsf_anomaly_foliage_poison(&rows); }
float detail_group_scale(const float* group) noexcept { return rsf_anomaly_foliage_scale(group); }

FoliageHistory::~FoliageHistory() { rsf_anomaly_foliage_drop(state_); }

bool FoliageHistory::begin_frame(temporal::ViewId view, std::uint64_t frame, std::uint64_t generation, std::uint32_t width,
    std::uint32_t height)
{
    if (!state_) state_ = rsf_anomaly_foliage_new();
    return rsf_anomaly_foliage_begin(state_, view, frame, generation, width, height);
}

bool FoliageHistory::active(temporal::ViewId view) const noexcept { return rsf_anomaly_foliage_active(state_, view); }
void FoliageHistory::reset_all() { rsf_anomaly_foliage_reset(state_); }

const TreeRecord* FoliageHistory::record_tree(temporal::ViewId view, std::uint64_t visual, const TreeRecord& current)
{
    return rsf_anomaly_foliage_tree(state_, view, visual, &current);
}

DetailRemap FoliageHistory::record_detail(temporal::ViewId view, std::uint64_t route, const FoliageConstants& current,
    const DetailRows& rows)
{
    DetailRemap result;
    rsf_anomaly_foliage_detail(state_, view, route, &current, &rows, &result);
    return result;
}

std::uint64_t FoliageHistory::conflicts() const noexcept { return rsf_anomaly_foliage_conflicts(state_); }
std::uint64_t FoliageHistory::table_overflows() const noexcept { return rsf_anomaly_foliage_overflows(state_); }

std::size_t FoliageHistory::recorded_items(temporal::ViewId view) const noexcept
{
    return rsf_anomaly_foliage_recorded(state_, view);
}
}
