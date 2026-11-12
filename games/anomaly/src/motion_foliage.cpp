// SPDX-License-Identifier: GPL-3.0-only
#include "motion_foliage.h"
#include <cmath>
#include <cstring>
#include <utility>

namespace rsf::anomaly::motion {
namespace {
std::size_t view_index(temporal::ViewId view) noexcept { return view == temporal::ViewId::Scope ? 1 : 0; }
std::uint32_t bits_of(float value) noexcept
{
    std::uint32_t bits;
    std::memcpy(&bits, &value, sizeof(bits));
    return bits;
}
std::size_t hash_key(const std::uint32_t (&word)[5]) noexcept
{
    std::uint64_t h = 1469598103934665603ull;
    for (const auto w : word) {
        h ^= w;
        h *= 1099511628211ull;
        h ^= h >> 29;
    }
    return static_cast<std::size_t>(h);
}
constexpr std::size_t initial_slots = 4096, maximum_slots = std::size_t{1} << 19;
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

bool detail_group_live(const float* group) noexcept
{
    for (std::size_t i = 0; i < 16; ++i)
        if (!std::isfinite(group[i])) return false;
    return bits_of(group[3]) != foliage_poison_word;
}

std::uint32_t count_live_groups(const DetailRows& rows) noexcept
{
    std::uint32_t live = 0;
    while (live < foliage_detail_groups && detail_group_live(rows.data() + live * 16)) ++live;
    return live;
}

void poison_detail_rows(DetailRows& rows) noexcept
{
    float poison;
    std::memcpy(&poison, &foliage_poison_word, sizeof(poison));
    rows.fill(poison);
}

float detail_group_scale(const float* group) noexcept
{
    const double x = group[0], y = group[1], z = group[2];
    return static_cast<float>(std::sqrt(x * x + y * y + z * z));
}

// ---- item table ----------------------------------------------------------------------------------------------------
void FoliageHistory::ItemTable::begin()
{
    used_ = 0;
    if (++serial_ == 0) {   // the 32-bit serial wrapped: forget every old stamp
        for (auto& slot : slots_) slot.stamp = 0;
        serial_ = 1;
    }
}

FoliageHistory::Item* FoliageHistory::ItemTable::find(const Key& key) noexcept
{
    if (slots_.empty()) return nullptr;
    const std::size_t mask = slots_.size() - 1;
    for (std::size_t n = 0, at = hash_key(key.word) & mask; n < slots_.size(); ++n, at = (at + 1) & mask) {
        auto& slot = slots_[at];
        if (slot.stamp != serial_) return nullptr;
        if (slot.key == key) return &slot;
    }
    return nullptr;
}

void FoliageHistory::ItemTable::grow()
{
    std::vector<Item> bigger(slots_.size() * 2);
    const std::size_t mask = bigger.size() - 1;
    for (const auto& slot : slots_) {
        if (slot.stamp != serial_) continue;
        std::size_t at = hash_key(slot.key.word) & mask;
        while (bigger[at].stamp == serial_) at = (at + 1) & mask;
        bigger[at] = slot;
    }
    slots_ = std::move(bigger);
}

FoliageHistory::Item* FoliageHistory::ItemTable::insert(const Key& key, bool& inserted)
{
    inserted = false;
    if (slots_.empty()) slots_.resize(initial_slots);
    if ((used_ + 1) * 2 > slots_.size() && slots_.size() < maximum_slots) grow();
    if ((used_ + 1) * 10 > slots_.size() * 9) return nullptr;
    const std::size_t mask = slots_.size() - 1;
    std::size_t at = hash_key(key.word) & mask;
    while (slots_[at].stamp == serial_) {
        if (slots_[at].key == key) return &slots_[at];
        at = (at + 1) & mask;
    }
    auto& slot = slots_[at];
    slot = Item{};
    slot.key = key;
    slot.stamp = serial_;
    ++used_;
    inserted = true;
    return &slot;
}

// ---- history -------------------------------------------------------------------------------------------------------
void FoliageHistory::Frame::begin(std::uint64_t f, std::uint64_t g, std::uint32_t w, std::uint32_t h)
{
    frame = f;
    generation = g;
    width = w;
    height = h;
    trees.clear();
    routes.clear();
    items.begin();
}

FoliageHistory::Key FoliageHistory::key_of(const float* group) noexcept
{
    // Translation of the instance (rows 0..2 .w), then the item's sun and hemisphere values (row 3 x and w).
    return {{bits_of(group[3]), bits_of(group[7]), bits_of(group[11]), bits_of(group[12]), bits_of(group[15])}};
}

bool FoliageHistory::begin_frame(temporal::ViewId id, std::uint64_t frame, std::uint64_t generation, std::uint32_t width,
    std::uint32_t height)
{
    auto& view = views_[view_index(id)];
    if (view.has_current) {
        const auto& now = view.now();
        if (now.frame == frame && now.generation == generation && now.width == width && now.height == height)
            return view.has_previous;   // a second phase_scene_begin of the same frame
    }
    const bool continuous = view.has_current && frame > view.now().frame && generation == view.now().generation &&
        width == view.now().width && height == view.now().height;
    if (continuous) view.current ^= 1;
    view.has_previous = continuous;
    view.now().begin(frame, generation, width, height);
    view.has_current = true;
    return view.has_previous;
}

bool FoliageHistory::active(temporal::ViewId id) const noexcept { return views_[view_index(id)].has_current; }

void FoliageHistory::reset_all() { views_ = {}; }

const TreeRecord* FoliageHistory::record_tree(temporal::ViewId id, std::uint64_t visual, const TreeRecord& current)
{
    auto& view = views_[view_index(id)];
    if (!view.has_current || !visual) return nullptr;
    auto& now = view.now();
    auto [entry, inserted] = now.trees.try_emplace(visual, TreeEntry{current, false});
    if (!inserted) {
        if (entry->second.conflict) return nullptr;
        if (!(entry->second.record == current)) {
            entry->second.conflict = true;
            ++conflicts_;
            return nullptr;
        }
    }
    if (!view.has_previous) return nullptr;
    const auto old = view.before().trees.find(visual);
    if (old == view.before().trees.end() || old->second.conflict) return nullptr;
    return &old->second.record;
}

DetailRemap FoliageHistory::record_detail(temporal::ViewId id, std::uint64_t route, const FoliageConstants& current,
    const DetailRows& rows)
{
    DetailRemap result;
    auto& view = views_[view_index(id)];
    if (!view.has_current) return result;
    auto& now = view.now();
    result.live = count_live_groups(rows);
    const auto [route_entry, route_new] = now.routes.try_emplace(route, RouteEntry{current, false});
    if (!route_new && !(route_entry->second.constants == current)) {
        route_entry->second.conflict = true;
        ++conflicts_;
    }
    Frame* previous = view.has_previous ? &view.before() : nullptr;
    if (previous) {
        const auto old = previous->routes.find(route);
        if (old != previous->routes.end() && !old->second.conflict) {
            result.route_found = true;
            result.previous = old->second.constants;
        }
    }
    for (std::uint32_t g = 0; g < result.live; ++g) {
        const float* group = rows.data() + static_cast<std::size_t>(g) * 16;
        const Key key = key_of(group);
        bool inserted = false;
        Item* mine = now.items.insert(key, inserted);
        if (!mine) {
            ++overflows_;
        } else if (inserted) {
            mine->route = route;
            std::memcpy(mine->rows, group, sizeof(mine->rows));
        } else if (mine->route != route || std::memcmp(mine->rows, group, sizeof(mine->rows)) != 0) {
            // The same instance twice in one frame with different state: neither copy may serve as history.
            if (!mine->conflict) { ++result.conflicts; ++conflicts_; }
            mine->conflict = true;
        }
        if (!result.route_found) { ++result.unmatched; continue; }
        const Item* old = previous->items.find(key);
        const float scale = old ? detail_group_scale(old->rows) : 0.0f;
        if (!old || old->conflict || old->route != route || !(scale > 0) || !std::isfinite(scale)) {
            ++result.unmatched;
            continue;
        }
        auto& item = result.items[g];
        item.valid = true;
        item.scale = scale;
        item.fade = 1.0f;   // fade is folded into the effective scale and cannot be separated from the rows
        std::memcpy(item.rows, old->rows, sizeof(item.rows));
        ++result.matched;
    }
    return result;
}

std::size_t FoliageHistory::recorded_items(temporal::ViewId id) const noexcept
{
    const auto& view = views_[view_index(id)];
    return view.has_current ? view.now().items.used() : 0;
}
}
