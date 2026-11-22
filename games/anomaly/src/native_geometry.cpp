// SPDX-License-Identifier: MIT
#include "native_geometry.h"
#include "build_profile.h"
#include "geometry_retirement.h"
#include <array>
#include <cstddef>
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include "native_memory.h"

struct VertexConstantTable {
    std::uintptr_t module;
    std::uint32_t table_rva, slots, size_offset, data_offset;
};
static_assert(sizeof(VertexConstantTable) == 24 && offsetof(VertexConstantTable, table_rva) == 8);
extern "C" bool rsf_anomaly_geometry_read_vertex(const VertexConstantTable*, std::uint32_t slot,
    std::uint32_t expected_size, std::uint32_t offset, void* out, std::uint32_t bytes,
    decltype(&rsf::anomaly::guarded_copy)) noexcept;
extern "C" bool rsf_anomaly_geometry_write_vertex(const VertexConstantTable*, std::uint32_t slot,
    std::uint32_t expected_size, std::uint32_t offset, const void* in, std::uint32_t bytes,
    decltype(&rsf::anomaly::guarded_copy)) noexcept;
extern "C" void rsf_anomaly_geometry_build_hook_sites(const rsf::anomaly::NativeSite*, void* const* replacements,
    void** originals, rsf::anomaly::HookSite*, std::size_t count) noexcept;

namespace rsf::anomaly::geometry {
namespace {
std::optional<InstanceKey> identity(std::uint64_t owner, std::uint64_t visual, std::uint64_t generation) noexcept
{
    if (!generation) return {};
    return InstanceKey{owner, visual, generation};
}
}

IdentityRegistry::IdentityRegistry() : state_(rsf_anomaly_geometry_registry_create()) {}
IdentityRegistry::~IdentityRegistry() { rsf_anomaly_geometry_registry_destroy(state_); }

std::optional<InstanceKey> IdentityRegistry::acquire(std::uint64_t owner, std::uint64_t visual)
{
    return identity(owner, visual, rsf_anomaly_geometry_register(state_, owner, visual));
}
std::optional<InstanceKey> IdentityRegistry::find(std::uint64_t owner, std::uint64_t visual) const
{
    return identity(owner, visual, rsf_anomaly_geometry_find(state_, owner, visual));
}
std::optional<InstanceKey> IdentityRegistry::retire(std::uint64_t owner, std::uint64_t visual)
{
    return identity(owner, visual, rsf_anomaly_geometry_retire(state_, owner, visual));
}
namespace {
using Engine = rsf_anomaly_geometry_engine;
using Matrix = rsf_anomaly_geometry_matrix;
using Context = rsf_anomaly_geometry_context;
using Visual = rsf_anomaly_geometry_visual;
using Window = rsf_anomaly_geometry_window;
static_assert(sizeof(Engine) == 16 && sizeof(Context) == 16 && sizeof(Window) == 32 && sizeof(Visual) == 80);
static_assert(sizeof(rsf_anomaly_geometry_facts) == 32 && sizeof(rsf_anomaly_geometry_platform) == 72 &&
    offsetof(rsf_anomaly_geometry_platform, bones) == 64);
static_assert(sizeof(GeometryStats) == 40 && sizeof(GeometryCoverage) == 1472);
static_assert(sizeof(rsf_anomaly_geometry_layout) == 16 && sizeof(rsf_anomaly_geometry_native_matrix) == 64);
static_assert(sizeof(rsf_anomaly_geometry_child_range) == 24);
static_assert(sizeof(rsf_anomaly_geometry_hierarchy) == 32 && offsetof(rsf_anomaly_geometry_hierarchy, read) == 24);
static_assert(sizeof(rsf_anomaly_geometry_writer) == 16 && sizeof(Matrix) == 128 &&
    alignof(Matrix) == alignof(double) && offsetof(Matrix, values) == 0);
static_assert(sizeof(HookSite) == 48 && offsetof(HookSite, expected) == 12 &&
    offsetof(HookSite, replacement) == 32 && offsetof(HookSite, original) == 40);
static_assert(sizeof(NativeSite) == 32 && offsetof(NativeSite, rva) == 8 && offsetof(NativeSite, expected) == 12);
static_assert(sizeof(UniqueGuard) == 24 && offsetof(UniqueGuard, rva) == 8 && offsetof(UniqueGuard, expected_hex) == 16);
static_assert(sizeof(rsf_anomaly_geometry_guard_profile) == 64 &&
    offsetof(rsf_anomaly_geometry_guard_profile, sites) == 24 &&
    offsetof(rsf_anomaly_geometry_guard_profile, unique) == 40 &&
    offsetof(rsf_anomaly_geometry_guard_profile, image_size) == 56);
static_assert(static_cast<std::uint8_t>(DrawKind::Rigid) == RSF_ANOMALY_GEOMETRY_RIGID);
static_assert(static_cast<std::uint8_t>(DrawKind::HardwareSkinned) == RSF_ANOMALY_GEOMETRY_HARDWARE_SKINNED);
static_assert(static_cast<std::uint8_t>(DrawKind::SingleBone) == RSF_ANOMALY_GEOMETRY_SINGLE_BONE);
static_assert(static_cast<std::uint8_t>(DrawKind::SoftwareSkinned) == RSF_ANOMALY_GEOMETRY_SOFTWARE_SKINNED);
static_assert(static_cast<std::uint8_t>(DrawKind::Tree) == RSF_ANOMALY_GEOMETRY_TREE);
static_assert(static_cast<std::uint8_t>(temporal::ViewId::Main) == RSF_ANOMALY_GEOMETRY_MAIN);
static_assert(static_cast<std::uint8_t>(temporal::ViewId::Scope) == RSF_ANOMALY_GEOMETRY_SCOPE);
static_assert(static_cast<std::uint8_t>(temporal::ProjectionDomain::World) == RSF_ANOMALY_GEOMETRY_WORLD);
static_assert(static_cast<std::uint8_t>(temporal::ProjectionDomain::FirstPerson) == RSF_ANOMALY_GEOMETRY_FIRST_PERSON);

template<class T>
std::optional<T> optional_enum(std::uint8_t value) noexcept
{
    if (value == RSF_ANOMALY_GEOMETRY_UNSET) return {};
    return static_cast<T>(value);
}

template<class T>
std::uint8_t encode_enum(std::optional<T> value) noexcept
{
    return value ? static_cast<std::uint8_t>(*value) : static_cast<std::uint8_t>(RSF_ANOMALY_GEOMETRY_UNSET);
}

Engine bind(const std::byte* module, const BuildProfile& build) noexcept
{
    return {reinterpret_cast<std::uintptr_t>(module), reinterpret_cast<std::uintptr_t>(&build)};
}

const BuildProfile& build_of(Engine engine) noexcept
{
    return *reinterpret_cast<const BuildProfile*>(engine.profile);
}

// Microsoft x64 C++ member ABI puts the visual's second argument in XMM1.
using Create = void*(__fastcall*)(void*, const char*, void*);
using Duplicate = void*(__fastcall*)(void*, void*);
using Delete = void(__fastcall*)(void*, void**, std::int32_t);
using Render = void(__fastcall*)(void*, float);
using Flush = void(__fastcall*)();
using Unary = void(__fastcall*)(void*);
std::array<void*, geometry_site_count> originals{};
HookSet native_hooks;

// Rust checks the image size, then every layout guard, then every detour entry, each with its unique prefix.
rsf_anomaly_geometry_guard_profile guard_profile(Engine engine) noexcept
{
    const auto& build = build_of(engine);
    const auto& profile = *build.geometry;
    return {guarded_copy, profile.geometry_layout_sites.data(), profile.geometry_layout_sites.size(),
        profile.geometry_sites.data(), profile.geometry_sites.size(), profile.geometry_unique_guards.data(),
        profile.geometry_unique_guards.size(), build.image_size()};
}

bool hook_install(Engine engine, std::uint32_t size) noexcept
{
    auto* module = reinterpret_cast<std::byte*>(engine.module);
    return native_hooks.install(module, size, native_geometry_hook_sites(build_of(engine)));
}

rsf_anomaly_geometry_hierarchy hierarchy(Engine engine) noexcept
{
    const auto& tables = build_of(engine).geometry->hierarchy_vtables;
    return {{tables[0], tables[1], tables[2], tables[3]}, hierarchy_children_offset, guarded_copy};
}

rsf_anomaly_geometry_layout layout(Engine engine) noexcept
{
    const auto& build = build_of(engine);
    return {build.geometry->render_phase_rva, build.geometry->second_viewport_rva,
        build.native->device_frame_rva, build.geometry->world_matrix_rva};
}

bool bones(std::uintptr_t first, std::size_t stride, std::uint32_t count,
    rsf_anomaly_geometry_native_matrix* out) noexcept
{
    for (std::uint32_t i = 0; i < count; ++i) {
        const auto address = first + std::size_t{i} * stride;
        if (!guarded_copy(&out[i], reinterpret_cast<const void*>(address), sizeof(*out))) return false;
    }
    return true;
}

constexpr rsf_anomaly_geometry_platform platform{
    guard_profile,
    []() noexcept { return native_hooks.created(); },
    hook_install,
    []() noexcept { return native_hooks.enable(); },
    []() noexcept { return native_hooks.disable(); },
    hierarchy,
    layout, guarded_copy, bones,
};

DrawClassification classification(std::uint8_t pass, std::uint8_t type, std::uint8_t verdict) noexcept
{
    return {static_cast<DrawPass>(pass), static_cast<VisualType>(type), static_cast<DrawVerdict>(verdict)};
}

// The facade supplies the existing C++ snapshot's storage. Rust decides when
// to sample, which rows are valid and when this copy is ready to publish.
using CaptureStorage = std::optional<DrawCapture>;
constexpr rsf_anomaly_geometry_writer writer{
    [](void* user, const Visual* scope, const Matrix* matrix, std::uint32_t count) noexcept -> Matrix* {
        auto& capture = static_cast<CaptureStorage*>(user)->emplace();
        capture.key = {scope->key.owner, scope->key.visual, scope->key.generation};
        capture.kind = static_cast<DrawKind>(scope->kind);
        capture.view = optional_enum<temporal::ViewId>(scope->view);
        capture.domain = optional_enum<temporal::ProjectionDomain>(scope->domain);
        if (matrix) capture.world = *matrix;
        try { capture.bones.resize(count); }
        catch (...) { return nullptr; }
        return capture.bones.data();
    },
    [](void* user, std::uint64_t layout) noexcept -> const DrawCapture* {
        auto& capture = **static_cast<CaptureStorage*>(user);
        if (!layout) capture.bones.clear();
        capture.palette_layout = layout;
        return &capture;
    },
};

void observe_draw_facts(DrawTimeFacts& facts, CaptureStorage& capture) noexcept
{
    rsf_anomaly_geometry_facts raw;
    const auto* kept = rsf_anomaly_geometry_observe(&platform, &writer, &capture, &raw);
    facts = {raw.in_visual, static_cast<DrawPass>(raw.pass), classification(raw.visual_pass, raw.type, raw.verdict),
        static_cast<DrawKind>(raw.kind), optional_enum<temporal::ViewId>(raw.view),
        optional_enum<temporal::ProjectionDomain>(raw.domain), kept, raw.visual,
        static_cast<VisualType>(raw.type), raw.tree_lod, raw.detail};
}

class FactoryScope {
public:
    FactoryScope() noexcept { rsf_anomaly_geometry_factory_enter(); }
    ~FactoryScope() { if (active_) rsf_anomaly_geometry_factory_leave(); }

    void complete(void* owner, void* result) noexcept
    {
        active_ = false;
        rsf_anomaly_geometry_factory_complete(&platform, reinterpret_cast<std::uintptr_t>(owner),
            reinterpret_cast<std::uintptr_t>(result));
    }

private:
    bool active_{true};
};

void* __fastcall create_hook(void* self, const char* name, void* reader)
{
    FactoryScope factory;
    auto* result = reinterpret_cast<Create>(originals[0])(self, name, reader);
    factory.complete(self, result);
    return result;
}

void* __fastcall child_hook(void* self, const char* name, void* reader)
{
    FactoryScope factory;
    auto* result = reinterpret_cast<Create>(originals[1])(self, name, reader);
    factory.complete(self, result);
    return result;
}

void* __fastcall duplicate_hook(void* self, void* source)
{
    FactoryScope factory;
    auto* result = reinterpret_cast<Duplicate>(originals[2])(self, source);
    factory.complete(self, result);
    return result;
}

void __fastcall delete_hook(void* self, void** reference, std::int32_t discard)
{
    rsf_anomaly_geometry_model_delete(&platform, reinterpret_cast<std::uintptr_t>(self), reinterpret_cast<std::uintptr_t>(reference));
    reinterpret_cast<Delete>(originals[3])(self, reference, discard);
}

void render_visual(std::size_t index, void* self, float lod, VisualType type)
{
    const DrawScope draw(self, type);
    reinterpret_cast<Render>(originals[index])(self, lod);
}

void __fastcall rigid_hook(void* self, float lod) { render_visual(4, self, lod, VisualType::Static); }
void __fastcall progressive_hook(void* self, float lod) { render_visual(5, self, lod, VisualType::Progressive); }
void __fastcall skeleton_st_hook(void* self, float lod) { render_visual(6, self, lod, VisualType::SkeletonST); }
void __fastcall skeleton_pm_hook(void* self, float lod) { render_visual(7, self, lod, VisualType::SkeletonPM); }
void __fastcall tree_st_hook(void* self, float lod) { render_visual(8, self, lod, VisualType::TreeST); }
void __fastcall tree_pm_hook(void* self, float lod) { render_visual(9, self, lod, VisualType::TreePM); }

void __fastcall flush_hook()
{
    DrawTimeFacts facts;
    CaptureStorage capture;
    observe_draw_facts(facts, capture);
    reinterpret_cast<Flush>(originals[geometry_site_flush])();
    // Native uploads are complete and the engine's D3D draw is next.
    rsf_anomaly_geometry_publish_flush(&facts);
}

void __fastcall render_hud_hook(void* self)
{
    struct HudScope {
        Context scope{};
        bool entered{rsf_anomaly_geometry_hud_enter(&scope)};
        ~HudScope() { if (entered) rsf_anomaly_geometry_context_leave(&scope); }
    } hud;
    reinterpret_cast<Unary>(originals[geometry_site_render_hud])(self);
}

struct HudWindow {
    Window scope{};
    HudWindow() noexcept { rsf_anomaly_geometry_window_enter(&scope); }
    ~HudWindow() { rsf_anomaly_geometry_window_leave(&scope); }
};

void __fastcall render_sorted_hook(void* self)
{
    HudWindow window;
    reinterpret_cast<Unary>(originals[geometry_site_render_sorted])(self);
}

void __fastcall render_emissive_hook(void* self)
{
    HudWindow window;
    reinterpret_cast<Unary>(originals[geometry_site_render_emissive])(self);
}

void __fastcall rm_near_hook(void* self)
{
    reinterpret_cast<Unary>(originals[geometry_site_rm_near])(self);
    rsf_anomaly_geometry_after_near(self);
}

void __fastcall rm_normal_hook(void* self)
{
    rsf_anomaly_geometry_near_leave();
    reinterpret_cast<Unary>(originals[geometry_site_rm_normal])(self);
}

void __fastcall detail_render_hook(void* self)
{
    struct DetailScope {
        DetailScope() noexcept { rsf_anomaly_geometry_detail_enter(&platform); }
        ~DetailScope() { rsf_anomaly_geometry_detail_leave(); }
    } detail;
    reinterpret_cast<Unary>(originals[geometry_site_detail_render])(self);
}

VertexConstantTable vertex_constants() noexcept
{
    const auto engine = rsf_anomaly_geometry_binding();
    if (!engine.module) return {};
    return {engine.module, build_of(engine).geometry->vertex_constant_buffers_rva,
        constant_buffer_slots, constant_buffer_size_offset, constant_buffer_data_offset};
}
}

DrawContextScope::DrawContextScope(std::optional<temporal::ViewId> view,
    std::optional<temporal::ProjectionDomain> domain) noexcept
{
    rsf_anomaly_geometry_context_enter(&scope_, encode_enum(view), encode_enum(domain), false);
}

DrawContextScope::DrawContextScope(InheritView, std::optional<temporal::ProjectionDomain> domain) noexcept
{
    rsf_anomaly_geometry_context_enter(&scope_, RSF_ANOMALY_GEOMETRY_UNSET, encode_enum(domain), true);
}

DrawContextScope::~DrawContextScope() { rsf_anomaly_geometry_context_leave(&scope_); }

DrawScope::DrawScope(const IdentityRegistry& registry, std::optional<InstanceKey> key, DrawKind kind,
    const float* world, const std::byte* skeleton, DrawClassification classification, std::uint64_t visual) noexcept
{
    scope_.registry = registry.state_;
    scope_.key = geometry_key(key.value_or(InstanceKey{}));
    scope_.world = reinterpret_cast<std::uintptr_t>(world);
    scope_.skeleton = reinterpret_cast<std::uintptr_t>(skeleton);
    scope_.visual = visual;
    scope_.kind = static_cast<std::uint8_t>(kind);
    scope_.pass = static_cast<std::uint8_t>(classification.pass);
    scope_.type = static_cast<std::uint8_t>(classification.type);
    scope_.verdict = static_cast<std::uint8_t>(classification.verdict);
    scope_.facade = &identity_;
    rsf_anomaly_geometry_visual_enter(&scope_);
    update_identity();
}

DrawScope::DrawScope(void* visual, VisualType type) noexcept
{
    scope_.facade = &identity_;
    rsf_anomaly_geometry_visual_source_enter(&platform, &scope_, reinterpret_cast<std::uintptr_t>(visual),
        static_cast<std::uint8_t>(type));
    update_identity();
}

void DrawScope::update_identity() noexcept
{
    identity_ = {{scope_.key.owner, scope_.key.visual, scope_.key.generation}, static_cast<DrawKind>(scope_.kind),
        optional_enum<temporal::ViewId>(scope_.view), optional_enum<temporal::ProjectionDomain>(scope_.domain),
        reinterpret_cast<const float*>(scope_.world), reinterpret_cast<const std::byte*>(scope_.skeleton),
        classification(scope_.pass, scope_.type, scope_.verdict), scope_.visual};
}

DrawScope::~DrawScope() { rsf_anomaly_geometry_visual_leave(&scope_); }

const DrawIdentity* borrow_draw() noexcept
{
    return static_cast<const DrawIdentity*>(rsf_anomaly_geometry_borrow_draw());
}

std::optional<DrawCapture> capture_current_draw() noexcept
{
    CaptureStorage capture;
    (void)rsf_anomaly_geometry_capture(&platform, &writer, &capture);
    return capture;
}

const char* pass_name(DrawPass pass) noexcept
{
    return rsf_anomaly_geometry_pass_name(static_cast<std::uint8_t>(pass));
}

const char* visual_type_name(VisualType type) noexcept
{
    return rsf_anomaly_geometry_type_name(static_cast<std::uint8_t>(type));
}

const char* verdict_name(DrawVerdict verdict) noexcept
{
    return rsf_anomaly_geometry_verdict_name(static_cast<std::uint8_t>(verdict));
}

void geometry_set_flush_callback(FlushCallback value) noexcept
{
    rsf_anomaly_geometry_set_flush(value);
}

void geometry_set_rm_near_callback(RmNearCallback value) noexcept
{
    rsf_anomaly_geometry_set_near(value);
}

void geometry_set_sink(GeometrySink value) noexcept
{
    rsf_anomaly_geometry_set_retired(reinterpret_cast<std::uintptr_t>(value.context), value.retired);
}

void geometry_set_draw_sink(DrawSink value) noexcept
{
    rsf_anomaly_geometry_set_draw(reinterpret_cast<std::uintptr_t>(value.context), value.draw);
}

void geometry_enable_foliage_identity(bool value) noexcept { rsf_anomaly_geometry_set_foliage(value); }
bool geometry_foliage_identity_enabled() noexcept { return rsf_anomaly_geometry_foliage(); }

bool read_vertex_constants(std::uint32_t slot, std::uint32_t expected_size, std::uint32_t offset, void* out,
    std::uint32_t bytes) noexcept
{
    const auto table = vertex_constants();
    return rsf_anomaly_geometry_read_vertex(&table, slot, expected_size, offset, out, bytes, guarded_copy);
}

bool write_vertex_constants(std::uint32_t slot, std::uint32_t expected_size, std::uint32_t offset, const void* in,
    std::uint32_t bytes) noexcept
{
    const auto table = vertex_constants();
    return rsf_anomaly_geometry_write_vertex(&table, slot, expected_size, offset, in, bytes, guarded_copy);
}

bool read_world_matrix(float (&out)[16]) noexcept
{
    return rsf_anomaly_geometry_read_world(&platform, out);
}

std::optional<temporal::ViewId> current_render_view(const std::byte* module, const BuildProfile& build) noexcept
{
    return optional_enum<temporal::ViewId>(rsf_anomaly_geometry_view(&platform, bind(module, build)));
}

RenderViewScope::RenderViewScope() noexcept
    : entered_(rsf_anomaly_geometry_render_enter(&platform, &scope_)) {}

RenderViewScope::~RenderViewScope()
{
    if (entered_) rsf_anomaly_geometry_context_leave(&scope_);
}

std::span<const HookSite> native_geometry_hook_sites(const BuildProfile& build) noexcept
{
    static std::array<HookSite, geometry_site_count> sites;
    const std::array<void*, geometry_site_count> replacements{reinterpret_cast<void*>(&create_hook),
        reinterpret_cast<void*>(&child_hook), reinterpret_cast<void*>(&duplicate_hook), reinterpret_cast<void*>(&delete_hook),
        reinterpret_cast<void*>(&rigid_hook), reinterpret_cast<void*>(&progressive_hook), reinterpret_cast<void*>(&skeleton_st_hook),
        reinterpret_cast<void*>(&skeleton_pm_hook), reinterpret_cast<void*>(&tree_st_hook), reinterpret_cast<void*>(&tree_pm_hook),
        reinterpret_cast<void*>(&flush_hook), reinterpret_cast<void*>(&render_hud_hook), reinterpret_cast<void*>(&render_sorted_hook),
        reinterpret_cast<void*>(&render_emissive_hook), reinterpret_cast<void*>(&rm_near_hook),
        reinterpret_cast<void*>(&rm_normal_hook), reinterpret_cast<void*>(&detail_render_hook)};
    rsf_anomaly_geometry_build_hook_sites(build.geometry->geometry_sites.data(), replacements.data(),
        originals.data(), sites.data(), sites.size());
    return sites;
}

bool install_native_geometry(std::byte* module, std::uint32_t image_size, const BuildProfile& build) noexcept
{
    return rsf_anomaly_geometry_install(&platform, bind(module, build), image_size);
}

bool disable_native_geometry() noexcept { return rsf_anomaly_geometry_disable(&platform); }
bool geometry_hooks_active() noexcept { return rsf_anomaly_geometry_active(); }
void geometry_retire_all() noexcept { rsf_anomaly_geometry_reset(); }

GeometryStats geometry_stats() noexcept
{
    GeometryStats result;
    rsf_anomaly_geometry_stats_read(&result);
    return result;
}

GeometryCoverage geometry_coverage() noexcept
{
    GeometryCoverage result;
    rsf_anomaly_geometry_coverage_read(&result);
    return result;
}

void geometry_report_coverage(void (*emit)(void*, const char*), void* user) noexcept
{
    if (!emit) return;
    struct Report {
        decltype(emit) emit_line;
        void* user;
    } report{emit, user};
    rsf_anomaly_geometry_report([](void* context, const char* line) noexcept {
        const auto& report = *static_cast<const Report*>(context);
        report.emit_line(report.user, line);
    }, &report);
}

#ifdef RSF_ANOMALY_GEOMETRY_TESTING
extern "C" bool rsf_test_anomaly_geometry_guard_lifecycle(std::byte*, const rsf_anomaly_geometry_platform*,
    rsf_anomaly_geometry_engine, rsf_anomaly_geometry_engine) noexcept;

bool geometry_test_guard_lifecycle(std::byte* module, const BuildProfile& build, const BuildProfile& other) noexcept
{
    return rsf_test_anomaly_geometry_guard_lifecycle(module, &platform, bind(module, build), bind(module, other));
}

void geometry_test_bind_module(std::byte* module, const BuildProfile& build) noexcept
{
    rsf_anomaly_geometry_test_bind(bind(module, build));
}
#endif
}
