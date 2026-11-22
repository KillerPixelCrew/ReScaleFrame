// SPDX-License-Identifier: MIT
#pragma once
#include "../temporal/history.h"
#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <string_view>
#include <vector>
#include "native_hooks.h"
#include "build_profile.h"
#include "native_geometry_platform.h"

namespace rsf::anomaly::geometry {
using temporal::InstanceKey;
enum class VisualLookup : std::uint8_t {
    Missing = RSF_ANOMALY_VISUAL_MISSING,
    Unique = RSF_ANOMALY_VISUAL_UNIQUE,
    Ambiguous = RSF_ANOMALY_VISUAL_AMBIGUOUS,
};
struct VisualProbe {
    VisualLookup lookup{VisualLookup::Missing};
    std::optional<InstanceKey> key; // Set only for Unique.
};
class IdentityRegistry {
public:
    IdentityRegistry();
    ~IdentityRegistry();
    IdentityRegistry(const IdentityRegistry&) = delete;
    IdentityRegistry& operator=(const IdentityRegistry&) = delete;
    std::optional<InstanceKey> acquire(std::uint64_t owner, std::uint64_t visual);
    std::optional<InstanceKey> find(std::uint64_t owner, std::uint64_t visual) const;
    std::optional<InstanceKey> find_visual(std::uint64_t visual) const;
    // Like find_visual, but distinguishes an unknown visual from one live under several owners.
    VisualProbe probe_visual(std::uint64_t visual) const;
    std::optional<InstanceKey> retire(std::uint64_t owner, std::uint64_t visual);
    std::size_t retire_all(void (*apply)(void*, InstanceKey), void* user);
private:
    friend class DrawScope;
    rsf_anomaly_geometry_registry* state_;
};

enum class DrawKind : std::uint8_t { Rigid, HardwareSkinned, SingleBone, SoftwareSkinned, Tree };

// Where the draw happens, read from CRender::phase at visual entry. Shadow-map and Z-fill passes write no motion.
enum class DrawPass : std::uint8_t {
    Main = RSF_ANOMALY_PASS_MAIN, Shadow = RSF_ANOMALY_PASS_SHADOW,
    Unknown = RSF_ANOMALY_PASS_UNKNOWN, Count = RSF_ANOMALY_PASS_COUNT,
};
// Which engine Render override the draw entered.
enum class VisualType : std::uint8_t {
    Static = RSF_ANOMALY_TYPE_STATIC, Progressive = RSF_ANOMALY_TYPE_PROGRESSIVE,
    SkeletonST = RSF_ANOMALY_TYPE_SKELETON_ST, SkeletonPM = RSF_ANOMALY_TYPE_SKELETON_PM,
    TreeST = RSF_ANOMALY_TYPE_TREE_ST, TreePM = RSF_ANOMALY_TYPE_TREE_PM, Count = RSF_ANOMALY_TYPE_COUNT,
};
enum class DrawVerdict : std::uint8_t {
    Identified = RSF_ANOMALY_DRAW_IDENTIFIED,
    IgnoredShadow = RSF_ANOMALY_DRAW_IGNORED_SHADOW,
    IgnoredStaticWorld = RSF_ANOMALY_DRAW_IGNORED_STATIC_WORLD,
    UnidentifiedDynamic = RSF_ANOMALY_DRAW_UNIDENTIFIED_DYNAMIC,
    UnidentifiedDeformed = RSF_ANOMALY_DRAW_UNIDENTIFIED_DEFORMED,
    UnidentifiedAmbiguous = RSF_ANOMALY_DRAW_UNIDENTIFIED_AMBIGUOUS,
    UnidentifiedUnreadable = RSF_ANOMALY_DRAW_UNIDENTIFIED_UNREADABLE,
    UnidentifiedNoCapture = RSF_ANOMALY_DRAW_UNIDENTIFIED_NO_CAPTURE,
    IdentifiedFoliage = RSF_ANOMALY_DRAW_IDENTIFIED_FOLIAGE,
    Count = RSF_ANOMALY_DRAW_COUNT,
};
const char* pass_name(DrawPass) noexcept;
const char* visual_type_name(VisualType) noexcept;
const char* verdict_name(DrawVerdict) noexcept;
struct DrawClassification {
    DrawPass pass{DrawPass::Main};
    VisualType type{VisualType::Static};
    DrawVerdict verdict{DrawVerdict::Identified};
};
struct DrawIdentity {
    InstanceKey key{};
    DrawKind kind{};
    std::optional<temporal::ViewId> view;
    std::optional<temporal::ProjectionDomain> domain;
    const float* world{}; // Borrowed native row-vector Fmatrix, read again at the actual D3D draw.
    const std::byte* skeleton{}; // Actual CSkeletonX subobject, not the complete visual.
    DrawClassification classification{};
    std::uint64_t visual{};       // Address of the visual being rendered (a level tree's identity).
};
struct DrawCapture {
    InstanceKey key{};
    DrawKind kind{};
    std::optional<temporal::ViewId> view;
    std::optional<temporal::ProjectionDomain> domain;
    std::optional<temporal::Mat4> world;
    std::uint64_t palette_layout{};
    std::vector<temporal::Mat4> bones;
};
struct DrawTimeFacts;
void observe_draw_facts(DrawTimeFacts&, std::optional<DrawCapture>&) noexcept;
class DrawContextScope {
public:
    struct InheritView {};
    DrawContextScope(std::optional<temporal::ViewId>, std::optional<temporal::ProjectionDomain>) noexcept;
    // Keeps the enclosing view and only changes the projection domain (first-person inside a main or scope frame).
    DrawContextScope(InheritView, std::optional<temporal::ProjectionDomain>) noexcept;
    ~DrawContextScope();
    DrawContextScope(const DrawContextScope&) = delete;
    DrawContextScope& operator=(const DrawContextScope&) = delete;
private:
    mutable rsf_anomaly_geometry_context scope_{};
};
class DrawScope {
public:
    DrawScope(const IdentityRegistry&, std::optional<InstanceKey>, DrawKind, const float*,
        const std::byte* skeleton = nullptr, DrawClassification = {}, std::uint64_t visual = 0) noexcept;
    DrawScope(void* visual, DrawKind, VisualType, const std::byte* skeleton) noexcept;
    ~DrawScope();
    DrawScope(const DrawScope&) = delete;
    DrawScope& operator=(const DrawScope&) = delete;
private:
    void enter() noexcept;
    rsf_anomaly_geometry_visual scope_{};
    DrawIdentity identity_{};
};
// Valid only until the matching visual returns, on this thread. No engine object/resource lease.
const DrawIdentity* borrow_draw() noexcept;
std::optional<DrawCapture> capture_current_draw() noexcept;
// Called by the draw-time hook before the engine's D3D draw. Counts the draw and, for an identified main-pass draw,
// captures it and hands it to the draw sink. Does nothing outside a DrawScope other than count.
void geometry_observe_draw() noexcept;

struct GeometrySink {
    void* context{};
    // Called synchronously after identity invalidation, before engine pooling/free or a new generation.
    // Runs on the engine lifecycle caller thread, not necessarily the render thread. Must not reenter.
    void (*retired)(void*, InstanceKey) noexcept{};
};
void geometry_set_sink(GeometrySink) noexcept; // Set only with native hooks quiescent.
struct DrawSink {
    void* context{};
    // Render thread, inside the engine's draw, synchronously. The capture is a copy; it holds no engine lease.
    // Must not throw or reenter the renderer.
    void (*draw)(void*, const DrawCapture&) noexcept{};
};
void geometry_set_draw_sink(DrawSink) noexcept; // Set only with native hooks quiescent.
// What the draw-time call site knows about one draw, handed to the flush callback.
struct DrawTimeFacts {
    bool in_visual{};                          // inside a hooked visual's Render (a DrawScope)
    DrawPass pass{DrawPass::Unknown};          // CRender::phase at the flush
    DrawClassification classification{};       // valid only with in_visual
    DrawKind kind{};                           // valid only with in_visual
    std::optional<temporal::ViewId> view;      // from the draw scope, else the enclosing context scope
    std::optional<temporal::ProjectionDomain> domain;
    const DrawCapture* capture{};              // the identified main-pass capture of this draw, else null
    // Foliage. A tree draw names its visual (stable address) and, for a progressive tree, the slide-window index it
    // draws with (tree_lod, -1 when not applicable or unreadable). A detail draw is any draw inside
    // CDetailManager::Render (grass batches), whatever the pass.
    std::uint64_t visual{};
    VisualType type{VisualType::Static};       // valid only with in_visual
    std::int64_t tree_lod{-1};
    bool detail{};
};
// Shared CBackend::constants.flush slot. Called on the render thread AFTER the original flush returned, i.e. after
// every native constant upload and state apply and immediately before the engine's D3D draw call, whether or not
// geometry capture is enabled. Must not throw or reenter the renderer. Null clears it.
using FlushCallback = void (*)(const DrawTimeFacts&) noexcept;
void geometry_set_flush_callback(FlushCallback) noexcept;
// Shared CRender::rmNear slot. The geometry producer owns the only detour on rmNear (geometry_site_rm_near); other
// adapter users (SR world window) register here instead of patching the same bytes a second time. Called on the
// render thread after the original rmNear returned, whether or not geometry capture is enabled. Null clears it.
using RmNearCallback = void (*)(void* render) noexcept;
void geometry_set_rm_near_callback(RmNearCallback) noexcept;
// Main or scope frame, from the engine's own IsSVPFrame predicate (isActive && dwFrame % frameDelay == 0).
// Empty when the bytes are unreadable. `module` is the executable base of the exact `build`.
std::optional<temporal::ViewId> current_render_view(const std::byte* module, const BuildProfile& build) noexcept;
// Opens the view/World context for one CRender::Render. Inert when geometry hooks are not active.
class RenderViewScope {
public:
    RenderViewScope() noexcept;
    ~RenderViewScope();
    RenderViewScope(const RenderViewScope&) = delete;
    RenderViewScope& operator=(const RenderViewScope&) = delete;
private:
    mutable rsf_anomaly_geometry_context scope_{};
    bool entered_{};
};
// Monotonic process counters for live validation. They prove hook invocation, not temporal correctness.
using GeometryStats = rsf_anomaly_geometry_stats;
GeometryStats geometry_stats() noexcept;

// Per-category coverage for the periodic log. Counts describe hook invocations, not temporal correctness.
struct GeometryCoverage : rsf_anomaly_geometry_coverage {
    static constexpr std::size_t passes = static_cast<std::size_t>(DrawPass::Count);
    static constexpr std::size_t types = static_cast<std::size_t>(VisualType::Count);
    static constexpr std::size_t verdicts = static_cast<std::size_t>(DrawVerdict::Count);
};
GeometryCoverage geometry_coverage() noexcept;
// Writes the coverage as log lines through `emit`. Rows with no draws are omitted.
void geometry_report_coverage(void (*emit)(void*, const char*), void*) noexcept;
// Foliage identity and constants. Trees are level visuals that never enter the model registry; with this enabled an
// unregistered FTreeVisual is classified IdentifiedFoliage (named by its address) instead of UnidentifiedDeformed.
// Off by default so the geometry hooks alone keep their earlier verdicts. Set only with native hooks quiescent.
void geometry_enable_foliage_identity(bool) noexcept;
bool geometry_foliage_identity_enabled() noexcept;
// Guarded access to the CPU copy the engine stages for vertex-stage constant buffer `slot` (the reflection's
// constant-buffer index of the shader bound for this draw). `expected_size` is that buffer's reflected size; the engine
// object must report the same size or nothing is read or written. Returns false on any mismatch or fault.
bool read_vertex_constants(std::uint32_t slot, std::uint32_t expected_size, std::uint32_t offset, void* out,
    std::uint32_t bytes) noexcept;
bool write_vertex_constants(std::uint32_t slot, std::uint32_t expected_size, std::uint32_t offset, const void* in,
    std::uint32_t bytes) noexcept;
// The world matrix in force for this draw (RCache.xforms.m_w), row-vector Fmatrix floats. False when unreadable.
bool read_world_matrix(float (&out)[16]) noexcept;
// The detour table of `build` (geometry_profile.h order). Valid until the next call.
std::span<const HookSite> native_geometry_hook_sites(const BuildProfile& build) noexcept;
// `build` is the profile selected for the running executable by its exact name and SHA-256 (build_profile.h);
// `image_size` must be that build's SizeOfImage. Every site and unique guard of that build is checked first.
bool install_native_geometry(std::byte* module, std::uint32_t image_size, const BuildProfile& build) noexcept;
bool disable_native_geometry() noexcept;
bool geometry_hooks_active() noexcept;
void geometry_retire_all() noexcept; // Device/session teardown. Generation never resets.
#ifdef RSF_ANOMALY_GEOMETRY_TESTING
void geometry_test_bind_module(std::byte*, const BuildProfile&) noexcept;
#endif
}
