// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "../temporal/history.h"
#include <cstddef>
#include <cstdint>
#include <map>
#include <mutex>
#include <optional>
#include <span>
#include <string_view>
#include <vector>
#include "native_hooks.h"

namespace rsf::anomaly::geometry {
using temporal::InstanceKey;
enum class VisualLookup : std::uint8_t { Missing, Unique, Ambiguous };
struct VisualProbe {
    VisualLookup lookup{VisualLookup::Missing};
    std::optional<InstanceKey> key; // Set only for Unique.
};
class IdentityRegistry {
public:
    std::optional<InstanceKey> acquire(std::uint64_t owner, std::uint64_t visual);
    std::optional<InstanceKey> find(std::uint64_t owner, std::uint64_t visual) const;
    std::optional<InstanceKey> find_visual(std::uint64_t visual) const;
    // Like find_visual, but distinguishes an unknown visual from one live under several owners.
    VisualProbe probe_visual(std::uint64_t visual) const;
    std::optional<InstanceKey> retire(std::uint64_t owner, std::uint64_t visual);
    std::vector<InstanceKey> retire_all();
private:
    mutable std::mutex mutex_;
    std::map<std::pair<std::uint64_t, std::uint64_t>, InstanceKey> live_;
    std::uint64_t generation_{};
};

enum class DrawKind : std::uint8_t { Rigid, HardwareSkinned, SingleBone, SoftwareSkinned, Tree };

// Where the draw happens, read from CRender::phase at visual entry. Shadow-map and Z-fill passes write no motion.
enum class DrawPass : std::uint8_t { Main, Shadow, Unknown, Count };
// Which engine Render override the draw entered.
enum class VisualType : std::uint8_t { Static, Progressive, SkeletonST, SkeletonPM, TreeST, TreePM, Count };
enum class DrawVerdict : std::uint8_t {
    Identified,             // A live registry identity names this visual.
    IgnoredShadow,          // Shadow-map or Z-fill phase: no motion output, so no identity is needed.
    IgnoredStaticWorld,     // Unregistered, rigid, world matrix exactly identity: level geometry, camera-only motion is exact.
    UnidentifiedDynamic,    // Unregistered skinned visual or non-identity world: needs an identity and has none.
    UnidentifiedDeformed,   // Unregistered tree: vertex-shader sway needs a deformation schema.
    UnidentifiedAmbiguous,  // The visual address is live under more than one owner.
    UnidentifiedUnreadable, // The world matrix could not be read to classify an unregistered draw.
    UnidentifiedNoCapture,  // Capture latched off, or the draw ran inside a model factory.
    IdentifiedFoliage,      // Unregistered tree visual named by its address (foliage identity enabled): the motion stage
                            // reads the draw's constants and keeps the previous frame's by that address.
    Count
};
struct DrawFacts {
    DrawPass pass{};
    VisualType type{};
    VisualLookup lookup{};
    bool capturing{true};       // Capture enabled and not inside a model factory.
    bool world_readable{};
    bool world_identity{};
    bool foliage_identity{};    // geometry_enable_foliage_identity(): an unregistered tree is identified by address.
};
// Pure classification. Only the unregistered rigid, non-shadow path depends on the world matrix.
DrawVerdict classify_draw(const DrawFacts&) noexcept;
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
    friend class DrawScope;
    friend void observe_draw_facts(DrawTimeFacts&, std::optional<DrawCapture>&) noexcept;
    std::optional<temporal::ViewId> view_;
    std::optional<temporal::ProjectionDomain> domain_;
    DrawContextScope* previous_{};
    static thread_local DrawContextScope* current_;
};
class DrawScope {
public:
    DrawScope(const IdentityRegistry&, std::optional<InstanceKey>, DrawKind, const float*,
        const std::byte* skeleton = nullptr, DrawClassification = {}, std::uint64_t visual = 0) noexcept;
    ~DrawScope();
    DrawScope(const DrawScope&) = delete;
    DrawScope& operator=(const DrawScope&) = delete;
private:
    friend const DrawIdentity* borrow_draw() noexcept;
    friend void geometry_observe_draw() noexcept;
    friend void observe_draw_facts(DrawTimeFacts&, std::optional<DrawCapture>&) noexcept;
    const IdentityRegistry& registry_;
    DrawIdentity identity_{};
    DrawScope* previous_{};
    static thread_local DrawScope* current_;
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
// Empty when the bytes are unreadable. `module` is the executable base.
std::optional<temporal::ViewId> current_render_view(const std::byte* module) noexcept;
// Opens the view/World context for one CRender::Render. Inert when geometry hooks are not active.
class RenderViewScope {
public:
    RenderViewScope() noexcept;
    ~RenderViewScope();
    RenderViewScope(const RenderViewScope&) = delete;
    RenderViewScope& operator=(const RenderViewScope&) = delete;
private:
    std::optional<DrawContextScope> scope_;
};
// Monotonic process counters for live validation. They prove hook invocation, not temporal correctness.
struct GeometryStats {
    std::uint64_t models_registered{}, models_retired{}, draws_identified{}, draws_unidentified{}, capture_faults{};
};
GeometryStats geometry_stats() noexcept;

// Per-category coverage for the periodic log. Counts describe hook invocations, not temporal correctness.
struct GeometryCoverage {
    static constexpr std::size_t passes = static_cast<std::size_t>(DrawPass::Count);
    static constexpr std::size_t types = static_cast<std::size_t>(VisualType::Count);
    static constexpr std::size_t verdicts = static_cast<std::size_t>(DrawVerdict::Count);
    std::uint64_t draws[passes][types][verdicts]{};   // Render-entry classification by pass, visual type and verdict.
    // Draw-time call site (CBackend::constants.flush: every draw or dispatch, hooked visual or not).
    std::uint64_t flush_total[passes]{}, flush_in_visual[passes]{};
    std::uint64_t captured{}, capture_stale{}, capture_no_world{}, capture_no_palette{}, capture_no_context{}, sunk{};
    // CDetailManager::Render windows and the draw-time flushes inside them, by pass.
    std::uint64_t detail_renders[passes]{}, flush_in_detail[passes]{};
    // Context producers.
    std::uint64_t views_main{}, views_scope{}, hud_scopes{}, near_windows{};
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
std::span<const HookSite> native_geometry_hook_sites() noexcept;
bool install_native_geometry(std::byte* module, std::uint32_t image_size,
    std::string_view verified_executable_sha256) noexcept;
bool disable_native_geometry() noexcept;
bool geometry_hooks_active() noexcept;
void geometry_retire_all() noexcept; // Device/session teardown. Generation never resets.
#ifdef RSF_ANOMALY_GEOMETRY_TESTING
void geometry_test_bind_module(std::byte*) noexcept;
#endif
}
