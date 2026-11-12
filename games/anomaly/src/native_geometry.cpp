// SPDX-License-Identifier: GPL-3.0-only
#include "native_geometry.h"
#include "geometry_profile.h"
#include <algorithm>
#include <array>
#include <atomic>
#include <cstring>
#include <limits>
#include <new>
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
namespace rsf::anomaly::geometry {
std::optional<InstanceKey> IdentityRegistry::acquire(std::uint64_t owner, std::uint64_t visual)
{
    if (!owner || !visual) return {};
    const std::lock_guard lock(mutex_);
    if (generation_ == std::numeric_limits<std::uint64_t>::max() || live_.contains({visual, owner})) return {};
    const InstanceKey key{owner, visual, ++generation_};
    live_[{visual, owner}] = key;
    return key;
}
std::optional<InstanceKey> IdentityRegistry::find(std::uint64_t owner, std::uint64_t visual) const
{
    const std::lock_guard lock(mutex_);
    const auto found = live_.find({visual, owner});
    return found == live_.end() ? std::nullopt : std::optional{found->second};
}
std::optional<InstanceKey> IdentityRegistry::find_visual(std::uint64_t visual) const
{
    const std::lock_guard lock(mutex_);
    const auto found = live_.lower_bound({visual, 0});
    if (found == live_.end() || found->first.first != visual) return {};
    const auto next = std::next(found);
    return next != live_.end() && next->first.first == visual ? std::nullopt : std::optional{found->second};
}
std::optional<InstanceKey> IdentityRegistry::retire(std::uint64_t owner, std::uint64_t visual)
{
    const std::lock_guard lock(mutex_);
    const auto found = live_.find({visual, owner});
    if (found == live_.end()) return {};
    const auto key = found->second;
    live_.erase(found);
    return key;
}
std::vector<InstanceKey> IdentityRegistry::retire_all()
{
    const std::lock_guard lock(mutex_);
    std::vector<InstanceKey> result;
    for (const auto& [slot, key] : live_) { (void)slot; result.push_back(key); }
    live_.clear();
    return result;
}

namespace {
bool safe_copy(void* output, const void* input, std::size_t size) noexcept
{
    if (!input) return false;
    __try { std::memcpy(output, input, size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
std::optional<temporal::Mat4> read_matrix(const float* input) noexcept
{
    float values[16];
    if (!safe_copy(values, input, sizeof(values))) return {};
    temporal::Mat4 matrix;
    for (std::size_t row = 0; row < 4; ++row)
        for (std::size_t col = 0; col < 4; ++col) matrix.at(row, col) = values[col * 4 + row];
    if (!temporal::finite(matrix) || matrix.at(3, 0) != 0 || matrix.at(3, 1) != 0 ||
        matrix.at(3, 2) != 0 || matrix.at(3, 3) != 1) return {};
    return matrix;
}
}
thread_local DrawContextScope* DrawContextScope::current_{};
DrawContextScope::DrawContextScope(std::optional<temporal::ViewId> view,
    std::optional<temporal::ProjectionDomain> domain) noexcept
    : view_(view), domain_(domain), previous_(current_)
{
    current_ = this;
}
DrawContextScope::~DrawContextScope() { current_ = previous_; }
thread_local DrawScope* DrawScope::current_{};
DrawScope::DrawScope(const IdentityRegistry& registry, std::optional<InstanceKey> key,
    DrawKind kind, const float* world, const std::byte* skeleton) noexcept : registry_(registry), previous_(current_)
{
    identity_.key = key.value_or(InstanceKey{});
    identity_.kind = kind;
    identity_.world = world;
    identity_.skeleton = skeleton;
    if (const auto context = DrawContextScope::current_) {
        identity_.view = context->view_;
        identity_.domain = context->domain_;
    }
    current_ = this;
}
DrawScope::~DrawScope() { current_ = previous_; }
const DrawIdentity* borrow_draw() noexcept
{
    const auto scope = DrawScope::current_;
    if (!scope) return nullptr;
    const auto& key = scope->identity_.key;
    return scope->registry_.find(key.owner, key.visual) == key ? &scope->identity_ : nullptr;
}
std::optional<DrawCapture> capture_current_draw() noexcept
{
    const auto identity = borrow_draw();
    if (!identity) return {};
    DrawCapture result;
    result.key = identity->key;
    result.kind = identity->kind;
    result.view = identity->view;
    result.domain = identity->domain;
    result.world = read_matrix(identity->world);
    if (identity->skeleton && identity->kind == DrawKind::HardwareSkinned) {
        std::uint16_t mode{};
        std::uint32_t count{};
        const std::byte* parent{};
        const std::byte* instances{};
        if (!safe_copy(&mode, identity->skeleton + 56, sizeof(mode)) || mode < 2 || mode > 5 ||
            !safe_copy(&count, identity->skeleton + 60, sizeof(count)) || count == 0 || count > 1024 ||
            !safe_copy(&parent, identity->skeleton + 8, sizeof(parent)) || !parent ||
            !safe_copy(&instances, parent + bone_instances_offset, sizeof(instances)) || !instances) return result;
        try {
            result.bones.reserve(count);
            for (std::uint32_t index = 0; index < count; ++index) {
                const auto matrix = read_matrix(reinterpret_cast<const float*>(
                    instances + index * bone_instance_stride + render_transform_offset));
                if (!matrix) { result.bones.clear(); return result; }
                result.bones.push_back(*matrix);
            }
            result.palette_layout = sbones_palette_layout;
        } catch (...) { result.bones.clear(); result.palette_layout = 0; }
    }
    return result;
}

namespace {
// Microsoft x64 C++ member ABI: RCX=this, RDX=name/source/ref, R8=reader/BOOL;
// the second argument float in Render is XMM1, never XMM0 or an integer register.
using Create = void*(__fastcall*)(void*, const char*, void*);
using Duplicate = void*(__fastcall*)(void*, void*);
using Delete = void(__fastcall*)(void*, void**, std::int32_t);
using Render = void(__fastcall*)(void*, float);
std::array<void*, 10> originals{};
IdentityRegistry native_identities;
std::mutex native_lifecycle;
GeometrySink sink;
std::atomic<std::byte*> native_module{};
std::atomic<bool> capture_enabled{};
std::atomic<bool> hooks_enabled{};
HookSet native_hooks;
std::atomic<std::uint64_t> models_registered{}, models_retired{}, draws_identified{}, draws_unidentified{}, capture_faults{};
// A faulted capture stays off until the next guarded install, rather than guessing which lifetimes were lost.
void fault() noexcept { capture_faults.fetch_add(1); capture_enabled = false; }
thread_local std::uint32_t factory_depth{};
struct FactoryScope {
    FactoryScope() { ++factory_depth; }
    ~FactoryScope() { --factory_depth; }
};

std::vector<std::uint64_t> visual_tree(void* root)
{
    std::vector<std::uint64_t> result;
    if (!root) return result;
    result.push_back(reinterpret_cast<std::uintptr_t>(root));
    const auto module = native_module.load();
    if (!module) return result;
    for (std::size_t i = 0; i < result.size(); ++i) {
        if (result.size() > 4096) throw std::bad_alloc{}; // Fail capture closed, leave engine untouched.
        const auto visual = reinterpret_cast<const std::byte*>(result[i]);
        const std::byte* vtable{};
        if (!safe_copy(&vtable, visual, sizeof(vtable))) throw std::bad_alloc{};
        bool hierarchy = false;
        for (const auto rva : hierarchy_vtables) hierarchy |= vtable == module + rva;
        if (!hierarchy) continue;
        std::int32_t borrowed_children{};
        if (!safe_copy(&borrowed_children, visual + 120, sizeof(borrowed_children))) throw std::bad_alloc{};
        if (borrowed_children) continue; // A linked level visual does not inherit this model's lifetime.
        std::uintptr_t range[3];
        if (!safe_copy(range, visual + hierarchy_children_offset, sizeof(range)) ||
            range[1] < range[0] || range[2] < range[1] || (range[1] - range[0]) % sizeof(void*) ||
            (range[1] - range[0]) / sizeof(void*) > 4096) throw std::bad_alloc{};
        for (auto item = range[0]; item < range[1]; item += sizeof(void*)) {
            std::uint64_t child{};
            if (!safe_copy(&child, reinterpret_cast<const void*>(item), sizeof(child)) || !child) throw std::bad_alloc{};
            if (std::find(result.begin(), result.end(), child) == result.end()) result.push_back(child);
        }
    }
    return result;
}
void retire_key(std::uint64_t owner, std::uint64_t visual)
{
    const auto key = native_identities.retire(owner, visual);
    if (!key) return;
    models_retired.fetch_add(1);
    if (sink.retired) sink.retired(sink.context, *key);
}
void model_returned(void* owner, void* visual) noexcept
{
    if (!capture_enabled.load() || !visual || factory_depth) return;
    try {
        const std::lock_guard lock(native_lifecycle);
        const auto tree = visual_tree(visual);
        const auto address = reinterpret_cast<std::uintptr_t>(owner);
        // Retire the entire pooled hierarchy before publishing ANY new generation.
        for (const auto item : tree) retire_key(address, item);
        for (const auto item : tree) {
            if (native_identities.acquire(address, item)) models_registered.fetch_add(1); else fault();
        }
    } catch (...) { fault(); }
}
void model_retiring(void* owner, void** reference) noexcept
{
    if (!capture_enabled.load()) return;
    try {
        void* visual{};
        if (!safe_copy(&visual, reference, sizeof(visual))) { fault(); return; }
        const std::lock_guard lock(native_lifecycle);
        for (const auto item : visual_tree(visual)) retire_key(reinterpret_cast<std::uintptr_t>(owner), item);
    } catch (...) { fault(); }
}
void* __fastcall create_hook(void* self, const char* name, void* reader)
{
    void* result;
    { const FactoryScope factory; result = reinterpret_cast<Create>(originals[0])(self, name, reader); }
    model_returned(self, result);
    return result;
}
void* __fastcall child_hook(void* self, const char* name, void* reader)
{
    void* result;
    { const FactoryScope factory; result = reinterpret_cast<Create>(originals[1])(self, name, reader); }
    model_returned(self, result);
    return result;
}
void* __fastcall duplicate_hook(void* self, void* source)
{
    void* result;
    { const FactoryScope factory; result = reinterpret_cast<Duplicate>(originals[2])(self, source); }
    model_returned(self, result);
    return result;
}
void __fastcall delete_hook(void* self, void** reference, std::int32_t discard)
{
    model_retiring(self, reference);
    reinterpret_cast<Delete>(originals[3])(self, reference, discard);
}
void render_visual(std::size_t index, void* self, float lod, DrawKind kind, std::uint32_t skeleton_offset = 0)
{
    std::optional<InstanceKey> key;
    if (capture_enabled.load() && !factory_depth) key = native_identities.find_visual(reinterpret_cast<std::uintptr_t>(self));
    const auto module = native_module.load();
    const auto skeleton = skeleton_offset ? static_cast<const std::byte*>(self) + skeleton_offset : nullptr;
    if (skeleton) {
        std::uint16_t mode{};
        if (safe_copy(&mode, skeleton + 56, sizeof(mode))) {
            kind = mode == 0 ? DrawKind::SoftwareSkinned : mode == 1 ? DrawKind::SingleBone : DrawKind::HardwareSkinned;
        }
    }
    (key ? draws_identified : draws_unidentified).fetch_add(1);
    DrawScope scope(native_identities, key, kind,
        module ? reinterpret_cast<const float*>(module + world_matrix_rva) : nullptr, skeleton);
    reinterpret_cast<Render>(originals[index])(self, lod);
}
void __fastcall rigid_hook(void* self, float lod) { render_visual(4, self, lod, DrawKind::Rigid); }
void __fastcall progressive_hook(void* self, float lod) { render_visual(5, self, lod, DrawKind::Rigid); }
void __fastcall skeleton_st_hook(void* self, float lod) { render_visual(6, self, lod, DrawKind::HardwareSkinned, skeleton_st_offset); }
void __fastcall skeleton_pm_hook(void* self, float lod) { render_visual(7, self, lod, DrawKind::HardwareSkinned, skeleton_pm_offset); }
void __fastcall tree_st_hook(void* self, float lod) { render_visual(8, self, lod, DrawKind::Tree); }
void __fastcall tree_pm_hook(void* self, float lod) { render_visual(9, self, lod, DrawKind::Tree); }
}
std::span<const HookSite> native_geometry_hook_sites() noexcept
{
    static const auto result = [] {
        const std::array<void*, 10> replacements{reinterpret_cast<void*>(&create_hook), reinterpret_cast<void*>(&child_hook),
            reinterpret_cast<void*>(&duplicate_hook), reinterpret_cast<void*>(&delete_hook), reinterpret_cast<void*>(&rigid_hook),
            reinterpret_cast<void*>(&progressive_hook), reinterpret_cast<void*>(&skeleton_st_hook), reinterpret_cast<void*>(&skeleton_pm_hook),
            reinterpret_cast<void*>(&tree_st_hook), reinterpret_cast<void*>(&tree_pm_hook)};
        std::array<HookSite, 10> sites;
        for (std::size_t i = 0; i < sites.size(); ++i)
            sites[i] = {geometry_sites[i].name, geometry_sites[i].rva, geometry_sites[i].expected, replacements[i], &originals[i]};
        return sites;
    }();
    return result;
}
void geometry_set_sink(GeometrySink value) noexcept
{
    const std::lock_guard lock(native_lifecycle);
    sink = value;
}
bool install_native_geometry(std::byte* module, std::uint32_t image_size, std::string_view hash) noexcept
{
    if (!module || image_size != native_image_size || hash != dx11_hash || hooks_enabled.load()) return false;
    for (const auto& site : geometry_layout_sites) {
        std::array<std::byte, 16> bytes;
        if (site.rva > image_size - bytes.size() || !safe_copy(bytes.data(), module + site.rva, bytes.size()) ||
            bytes != site.expected) return false;
    }
    for (const auto& site : geometry_sites) {
        std::array<std::byte, 16> bytes;
        if (site.rva > image_size - bytes.size() || !safe_copy(bytes.data(), module + site.rva, bytes.size()) ||
            bytes != site.expected) return false;
    }
    if (native_hooks.created() && native_module.load() != module) return false;
    geometry_retire_all(); // A new capture epoch never inherits identities that missed deletes while disabled.
    native_module = module;
    capture_enabled = true;
    const bool installed = native_hooks.created() ? native_hooks.enable()
        : native_hooks.install(module, image_size, native_geometry_hook_sites());
    hooks_enabled = installed;
    if (!installed) capture_enabled = false;
    return installed;
}
bool disable_native_geometry() noexcept
{
    capture_enabled = false;
    if (!native_hooks.created()) return false;
    const auto disabled = native_hooks.disable();
    if (disabled) hooks_enabled = false;
    geometry_retire_all(); // Deletes are no longer observed, so no identity may outlive the epoch.
    return disabled;
}
bool geometry_hooks_active() noexcept { return hooks_enabled.load() && capture_enabled.load(); }
void geometry_retire_all() noexcept
{
    try {
        const std::lock_guard lock(native_lifecycle);
        for (const auto key : native_identities.retire_all()) {
            models_retired.fetch_add(1);
            if (sink.retired) sink.retired(sink.context, key);
        }
    } catch (...) { fault(); }
}
GeometryStats geometry_stats() noexcept
{
    return {models_registered.load(), models_retired.load(), draws_identified.load(), draws_unidentified.load(),
        capture_faults.load()};
}
#ifdef RSF_ANOMALY_GEOMETRY_TESTING
void geometry_test_bind_module(std::byte* module) noexcept { native_module = module; capture_enabled = true; }
#endif
}
