// SPDX-License-Identifier: MIT
#include "motion_foliage.h"

namespace rsf::anomaly::motion {
static_assert(sizeof(TreeRecord) == 104 && offsetof(TreeRecord, lod) == 96);
static_assert(sizeof(DetailRemap) == 4704 && offsetof(DetailRemap, previous) == 20 && offsetof(DetailRemap, items) == 68);
static_assert(sizeof(DetailRows) == 3904);

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
}

FoliageLayout make_foliage_layout(Family family, const Reflection& stock) noexcept
{
    FoliageLayout layout;
    layout.family = family;
    const auto find = [&](const char* name) {
        ConstantRef ref;
        for (std::size_t b = 0; b < stock.buffers.size(); ++b)
            for (const auto& variable : stock.buffers[b].variables)
                if (variable.name == name) {
                    ref = {true, static_cast<std::uint32_t>(b), stock.buffers[b].size, variable.offset, variable.size};
                    return ref;
                }
        return ref;
    };
    const auto inside = [](const ConstantRef& ref) {
        return !ref.present || (ref.size && ref.offset <= ref.buffer_size && ref.size <= ref.buffer_size - ref.offset);
    };
    if (family == Family::FoliageTree) {
        layout.xform = find("m_xform");
        layout.consts = find("consts");
        layout.wave = find("wave");
        layout.wind = find("wind");
        layout.usable = layout.xform.present && layout.xform.size >= tree_xform_floats * sizeof(float) &&
            layout.consts.size <= 16 && layout.wave.size <= 16 && layout.wind.size <= 16 &&
            inside(layout.xform) && inside(layout.consts) && inside(layout.wave) && inside(layout.wind);
    } else if (family == Family::FoliageDetail) {
        layout.consts = find("consts");
        layout.wave = find("wave");
        layout.wind = find("dir2D");
        layout.array = find("array");
        layout.usable = layout.array.present && layout.array.size == foliage_detail_floats * sizeof(float) &&
            layout.consts.size <= 16 && layout.wave.size <= 16 && layout.wind.size <= 16 &&
            inside(layout.array) && inside(layout.consts) && inside(layout.wave) && inside(layout.wind);
    }
    return layout;
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
