// SPDX-License-Identifier: MIT
#pragma once
// Tree (FTreeVisual) and detail/grass (CDetailManager) previous-frame state for the foliage VS b13 buffer.
// Pure CPU: no engine memory, no D3D. The motion stage reads each foliage draw's constants from the engine's staged VS
// constant buffer after CBackend::constants.flush (native_geometry.h read_vertex_constants), and this keeps them per
// stable identity so the next frame can bind the previous frame's wind, time and instance state.
//
// Identity
// * A tree is its visual's address. A level tree is never destroyed during a level and its m_xform is constant, so the
//   previous record must carry the same xform bytes; a recycled address after a level change therefore never matches.
// * A grass instance has no pointer in the constant buffer, so its identity is the engine's own per-item data that does
//   not change with fade or time: the world translation (rows 0..2 .w) and the item's sun and hemisphere values (row 3).
//   Batch slot, batch number and upload order are never identity. A previous row is bound to the CURRENT group of the
//   same item, and only when both frames drew it through the same route (shader permutation and wave set), because the
//   previous wave/wind constants are per route.
// Same frame rules as MotionHistory: only the last recorded frame of the same view contributes, a reset
// generation or a size change drops everything, and a same-frame conflict leaves no history.
#include "motion_constants.h"
#include "motion_history.h"
#include "shader_loader.h"
#include <array>
#include <cstddef>
#include <cstdint>

namespace rsf::anomaly::motion {
// One variable of a stock shader's constant buffer. `buffer` is the reflection's constant-buffer index, which is also
// the engine's CBackend::m_aVertexConstants index.
struct ConstantRef {
    bool present{};
    std::uint32_t buffer{}, buffer_size{}, offset{}, size{};
};
struct FoliageLayout {
    Family family{};
    ConstantRef xform, consts, wave, wind, array;
    bool usable{};   // everything this family's history needs is there
};
// Variable names are the shader source's: tree m_xform consts wave wind; detail consts wave dir2D array. A variable the
// compiler removed (USE_TREEWAVE zeroes the sway, so wave/wind/consts can vanish) is simply not present.
FoliageLayout make_foliage_layout(Family, const Reflection& stock) noexcept;

inline constexpr std::size_t tree_xform_floats = 12;

struct TreeRecord {
    float xform[tree_xform_floats]{};
    FoliageConstants constants;
    std::int64_t lod{-1};   // progressive tree slide window; -1 for a stripified tree
    bool operator==(const TreeRecord&) const = default;
};

// The current batch as the engine staged it: four float4 rows per group.
using DetailRows = std::array<float, foliage_detail_floats>;
// Leading live groups of a batch (a real batch fills groups 0..n-1).
std::uint32_t count_live_groups(const DetailRows&) noexcept;
// Fills every word of the batch with the poison pattern (what write_vertex_constants writes back).
void poison_detail_rows(DetailRows&) noexcept;
// Effective scale of a group: the length of the first row's xyz (mRotY is a rotation plus a translation).
float detail_group_scale(const float* group) noexcept;

struct DetailRemap {
    std::uint32_t live{}, matched{}, unmatched{}, conflicts{};
    bool route_found{};              // the previous frame drew this route
    FoliageConstants previous{};     // its consts/wave/dir2D
    std::array<FoliageItem, foliage_detail_groups> items{};
};

struct FoliageHistoryState;
class FoliageHistory {
public:
    FoliageHistory() = default;
    ~FoliageHistory();
    FoliageHistory(const FoliageHistory&) = delete;
    FoliageHistory& operator=(const FoliageHistory&) = delete;
    // Starts `frame` of `view`. Returns true when the previous frame stays usable.
    bool begin_frame(temporal::ViewId, std::uint64_t frame, std::uint64_t generation, std::uint32_t width, std::uint32_t height);
    bool active(temporal::ViewId) const noexcept;
    void reset_all();
    // Records `current` for the active frame; returns the previous frame's record of the same visual, or null (not drawn
    // last frame, or a same-frame conflict). Consume before the next history call.
    const TreeRecord* record_tree(temporal::ViewId, std::uint64_t visual, const TreeRecord& current);
    // Records the batch and remaps the previous frame's matching instances to its current groups.
    DetailRemap record_detail(temporal::ViewId, std::uint64_t route, const FoliageConstants& current, const DetailRows& rows);
    std::uint64_t conflicts() const noexcept;
    std::uint64_t table_overflows() const noexcept;
    // For tests: instances recorded in the active frame of `view`.
    std::size_t recorded_items(temporal::ViewId) const noexcept;
private:
    FoliageHistoryState* state_{};
};
}
