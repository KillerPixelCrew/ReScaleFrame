// SPDX-License-Identifier: MIT
#include "native_geometry.h"
#include "build_profile.h"
#include "geometry_retirement.h"
#include <algorithm>
#include <array>
#include <cstring>
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
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
std::optional<InstanceKey> IdentityRegistry::find_visual(std::uint64_t visual) const
{
    return probe_visual(visual).key;
}
VisualProbe IdentityRegistry::probe_visual(std::uint64_t visual) const
{
    rsf_anomaly_geometry_probe result{};
    rsf_anomaly_geometry_probe_visual(state_, visual, &result);
    return {static_cast<VisualLookup>(result.lookup),
        identity(result.key.owner, result.key.visual, result.key.generation)};
}
std::optional<InstanceKey> IdentityRegistry::retire(std::uint64_t owner, std::uint64_t visual)
{
    return identity(owner, visual, rsf_anomaly_geometry_retire(state_, owner, visual));
}
std::size_t IdentityRegistry::retire_all(void (*apply)(void*, InstanceKey), void* user)
{
    RetirementCall call{apply, user};
    const auto count = rsf_anomaly_geometry_retire_all(state_, apply ? RetirementCall::visit : nullptr, &call);
    if (call.exception) std::rethrow_exception(call.exception);
    return count;
}


namespace {
using Engine = rsf_anomaly_geometry_engine;
using Matrix = rsf_anomaly_geometry_matrix;
using Context = rsf_anomaly_geometry_context;
using Visual = rsf_anomaly_geometry_visual;
using Window = rsf_anomaly_geometry_window;
static_assert(sizeof(Engine) == 16 && sizeof(Context) == 16 && sizeof(Window) == 32 && sizeof(Visual) == 80);
static_assert(sizeof(rsf_anomaly_geometry_facts) == 32 && sizeof(rsf_anomaly_geometry_platform) == 128);
static_assert(sizeof(GeometryStats) == 40 && sizeof(GeometryCoverage) == 1472);
static_assert(sizeof(rsf_anomaly_geometry_palette) == 24 && sizeof(rsf_anomaly_geometry_view_sample) == 8);
static_assert(sizeof(rsf_anomaly_geometry_child_range) == 24 && sizeof(rsf_anomaly_geometry_callback) == 24);
static_assert(sizeof(rsf_anomaly_geometry_writer) == 24 && sizeof(Matrix) == 128);
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

bool safe_copy(void* output, const void* input, std::size_t size) noexcept
{
    if (!input) return false;
    __try { std::memcpy(output, input, size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

template<class T>
bool read(std::uintptr_t address, T& value) noexcept
{
    return safe_copy(&value, reinterpret_cast<const void*>(address), sizeof(T));
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

bool guard(Engine engine, std::uint32_t image_size) noexcept
{
    if (!engine.module || image_size != build_of(engine).image_size()) return false;
    const auto* module = reinterpret_cast<const std::byte*>(engine.module);
    const auto& profile = *build_of(engine).geometry;
    const auto guarded = [&](const NativeSite& site) {
        std::array<std::byte, 16> bytes;
        return site.rva <= image_size - bytes.size() &&
            safe_copy(bytes.data(), module + site.rva, bytes.size()) && bytes == site.expected &&
            unique_guards_match(module, image_size, profile.geometry_unique_guards, site.rva);
    };
    for (const auto& site : profile.geometry_layout_sites) if (!guarded(site)) return false;
    for (const auto& site : profile.geometry_sites) if (!guarded(site)) return false;
    return true;
}

bool hook_install(Engine engine, std::uint32_t size) noexcept
{
    auto* module = reinterpret_cast<std::byte*>(engine.module);
    return native_hooks.install(module, size, native_geometry_hook_sites(build_of(engine)));
}

bool children(Engine engine, std::uintptr_t visual, rsf_anomaly_geometry_child_range* range) noexcept
{
    std::uintptr_t vtable{};
    if (!read(visual, vtable)) return false;
    const auto& tables = build_of(engine).geometry->hierarchy_vtables;
    if (std::none_of(tables.begin(), tables.end(), [&](auto rva) { return vtable == engine.module + rva; }))
        return true;
    std::int32_t borrowed{};
    return read(visual + hierarchy_children_offset + sizeof(*range), borrowed) &&
        (borrowed || read(visual + hierarchy_children_offset, *range));
}

std::uint8_t pass(Engine engine) noexcept
{
    std::uint32_t phase{};
    if (read(engine.module + build_of(engine).geometry->render_phase_rva, phase)) {
        if (phase == phase_normal) return RSF_ANOMALY_PASS_MAIN;
        if (phase == phase_smap) return RSF_ANOMALY_PASS_SHADOW;
    }
    return RSF_ANOMALY_PASS_UNKNOWN;
}

bool view(Engine engine, rsf_anomaly_geometry_view_sample* sample) noexcept
{
    const auto& build = build_of(engine);
    const auto viewport = engine.module + build.geometry->second_viewport_rva;
    return read(viewport, sample->active) && read(viewport + second_viewport_delay_offset, sample->delay) &&
        read(engine.module + build.native->device_frame_rva, sample->frame);
}

std::uintptr_t world(Engine engine) noexcept
{
    return engine.module ? engine.module + build_of(engine).geometry->world_matrix_rva : 0;
}

bool skin_mode(std::uintptr_t skeleton, std::uint16_t* mode) noexcept
{
    return read(skeleton + 56, *mode);
}

bool palette(std::uintptr_t skeleton, rsf_anomaly_geometry_palette* sample) noexcept
{
    sample->layout = sbones_palette_layout;
    return skin_mode(skeleton, &sample->mode) && read(skeleton + 60, sample->count);
}

std::uintptr_t bone_instances(std::uintptr_t skeleton) noexcept
{
    std::uintptr_t parent{}, instances{};
    return read(skeleton + 8, parent) && parent && read(parent + bone_instances_offset, instances) ? instances : 0;
}

bool matrix(std::uintptr_t address, float* out) noexcept
{
    return safe_copy(out, reinterpret_cast<const void*>(address), sizeof(float) * 16);
}

bool bone(std::uintptr_t instances, std::uint32_t index, float* out) noexcept
{
    return matrix(instances + index * bone_instance_stride + render_transform_offset, out);
}

std::int64_t tree_lod(std::uintptr_t visual) noexcept
{
    std::uint32_t lod{};
    return read(visual + tree_pm_last_lod_offset, lod) ? static_cast<std::int64_t>(lod) : -1;
}

constexpr rsf_anomaly_geometry_platform platform{
    guard,
    []() noexcept { return native_hooks.created(); },
    hook_install,
    []() noexcept { return native_hooks.enable(); },
    []() noexcept { return native_hooks.disable(); },
    children,
    [](std::uintptr_t address, std::uintptr_t* result) noexcept { return read(address, *result); },
    pass, view, world, matrix, skin_mode, palette, bone_instances, bone, tree_lod,
};

DrawClassification classification(std::uint8_t pass, std::uint8_t type, std::uint8_t verdict) noexcept
{
    return {static_cast<DrawPass>(pass), static_cast<VisualType>(type), static_cast<DrawVerdict>(verdict)};
}

temporal::Mat4 copy_matrix(const Matrix& input) noexcept
{
    temporal::Mat4 result;
    std::copy_n(input.values, result.values.size(), result.values.begin());
    return result;
}

// The facade supplies the existing C++ snapshot's storage. Rust decides when
// to sample, which rows are valid and when this copy is ready to publish.
using CaptureStorage = std::optional<DrawCapture>;
constexpr rsf_anomaly_geometry_writer writer{
    [](void* user, const Visual* scope, const Matrix* matrix, std::uint32_t count) noexcept {
        auto& capture = static_cast<CaptureStorage*>(user)->emplace();
        capture.key = {scope->key.owner, scope->key.visual, scope->key.generation};
        capture.kind = static_cast<DrawKind>(scope->kind);
        capture.view = optional_enum<temporal::ViewId>(scope->view);
        capture.domain = optional_enum<temporal::ProjectionDomain>(scope->domain);
        if (matrix) capture.world = copy_matrix(*matrix);
        try { capture.bones.reserve(count); return true; }
        catch (...) { return false; }
    },
    [](void* user, std::uint32_t, const Matrix* matrix) noexcept {
        (**static_cast<CaptureStorage*>(user)).bones.push_back(copy_matrix(*matrix));
    },
    [](void* user, std::uint64_t layout) noexcept -> const void* {
        auto& capture = **static_cast<CaptureStorage*>(user);
        if (!layout) capture.bones.clear();
        capture.palette_layout = layout;
        return &capture;
    },
};

struct FactoryScope {
    FactoryScope() noexcept { rsf_anomaly_geometry_factory_enter(); }
    ~FactoryScope() { rsf_anomaly_geometry_factory_leave(); }
};

void* __fastcall create_hook(void* self, const char* name, void* reader)
{
    void* result;
    { const FactoryScope factory; result = reinterpret_cast<Create>(originals[0])(self, name, reader); }
    rsf_anomaly_geometry_model_return(&platform, reinterpret_cast<std::uintptr_t>(self), reinterpret_cast<std::uintptr_t>(result));
    return result;
}

void* __fastcall child_hook(void* self, const char* name, void* reader)
{
    void* result;
    { const FactoryScope factory; result = reinterpret_cast<Create>(originals[1])(self, name, reader); }
    rsf_anomaly_geometry_model_return(&platform, reinterpret_cast<std::uintptr_t>(self), reinterpret_cast<std::uintptr_t>(result));
    return result;
}

void* __fastcall duplicate_hook(void* self, void* source)
{
    void* result;
    { const FactoryScope factory; result = reinterpret_cast<Duplicate>(originals[2])(self, source); }
    rsf_anomaly_geometry_model_return(&platform, reinterpret_cast<std::uintptr_t>(self), reinterpret_cast<std::uintptr_t>(result));
    return result;
}

void __fastcall delete_hook(void* self, void** reference, std::int32_t discard)
{
    rsf_anomaly_geometry_model_delete(&platform, reinterpret_cast<std::uintptr_t>(self), reinterpret_cast<std::uintptr_t>(reference));
    reinterpret_cast<Delete>(originals[3])(self, reference, discard);
}

void render_visual(std::size_t index, void* self, float lod, DrawKind kind, VisualType type, std::uint32_t skeleton_offset = 0)
{
    const auto* skeleton = skeleton_offset ? static_cast<const std::byte*>(self) + skeleton_offset : nullptr;
    const DrawScope draw(self, kind, type, skeleton);
    reinterpret_cast<Render>(originals[index])(self, lod);
}

void __fastcall rigid_hook(void* self, float lod) { render_visual(4, self, lod, DrawKind::Rigid, VisualType::Static); }
void __fastcall progressive_hook(void* self, float lod) { render_visual(5, self, lod, DrawKind::Rigid, VisualType::Progressive); }
void __fastcall skeleton_st_hook(void* self, float lod) { render_visual(6, self, lod, DrawKind::HardwareSkinned, VisualType::SkeletonST, skeleton_st_offset); }
void __fastcall skeleton_pm_hook(void* self, float lod) { render_visual(7, self, lod, DrawKind::HardwareSkinned, VisualType::SkeletonPM, skeleton_pm_offset); }
void __fastcall tree_st_hook(void* self, float lod) { render_visual(8, self, lod, DrawKind::Tree, VisualType::TreeST); }
void __fastcall tree_pm_hook(void* self, float lod) { render_visual(9, self, lod, DrawKind::Tree, VisualType::TreePM); }

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
    rsf_anomaly_geometry_near_enter();
    rsf_anomaly_geometry_publish_near(self);
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

// The dx10ConstantBuffer behind CBackend::m_aVertexConstants[slot], only when it reports the reflected size.
std::uintptr_t vertex_constant_data(std::uint32_t slot, std::uint32_t expected_size, std::uint32_t offset,
    std::uint32_t bytes) noexcept
{
    const auto engine = rsf_anomaly_geometry_binding();
    if (!engine.module || slot >= constant_buffer_slots || !expected_size || bytes > expected_size ||
        offset > expected_size - bytes) return 0;
    std::uintptr_t object{}, data{};
    std::uint32_t size{};
    if (!read(engine.module + build_of(engine).geometry->vertex_constant_buffers_rva + slot * sizeof(object), object) ||
        !object || !read(object + constant_buffer_size_offset, size) || size != expected_size ||
        !read(object + constant_buffer_data_offset, data) || !data) return 0;
    return data + offset;
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
    enter();
}

DrawScope::DrawScope(void* visual, DrawKind kind, VisualType type, const std::byte* skeleton) noexcept
{
    scope_.visual = reinterpret_cast<std::uintptr_t>(visual);
    scope_.skeleton = reinterpret_cast<std::uintptr_t>(skeleton);
    scope_.kind = static_cast<std::uint8_t>(kind);
    scope_.type = static_cast<std::uint8_t>(type);
    rsf_anomaly_geometry_classify_visual(&platform, &scope_);
    enter();
}

void DrawScope::enter() noexcept
{
    scope_.facade = &identity_;
    rsf_anomaly_geometry_visual_enter(&scope_);
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

DrawVerdict classify_draw(const DrawFacts& facts) noexcept
{
    const rsf_anomaly_geometry_draw draw{static_cast<std::int32_t>(facts.pass), static_cast<std::int32_t>(facts.type),
        static_cast<std::int32_t>(facts.lookup), facts.capturing, facts.world_readable, facts.world_identity,
        facts.foliage_identity};
    return static_cast<DrawVerdict>(rsf_anomaly_geometry_classify(&draw));
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

void observe_draw_facts(DrawTimeFacts& facts, CaptureStorage& capture) noexcept
{
    rsf_anomaly_geometry_facts raw;
    const auto* kept = rsf_anomaly_geometry_observe(&platform, &writer, &capture, &raw);
    facts = {raw.in_visual, static_cast<DrawPass>(raw.pass), classification(raw.visual_pass, raw.type, raw.verdict),
        static_cast<DrawKind>(raw.kind), optional_enum<temporal::ViewId>(raw.view),
        optional_enum<temporal::ProjectionDomain>(raw.domain), static_cast<const DrawCapture*>(kept), raw.visual,
        static_cast<VisualType>(raw.type), raw.tree_lod, raw.detail};
}

void geometry_observe_draw() noexcept
{
    DrawTimeFacts facts;
    CaptureStorage capture;
    observe_draw_facts(facts, capture);
}

void geometry_set_flush_callback(FlushCallback value) noexcept
{
    rsf_anomaly_geometry_set_flush({reinterpret_cast<rsf_anomaly_geometry_fn>(value), 0,
        [](rsf_anomaly_geometry_fn fn, std::uintptr_t, const void* facts) noexcept {
            reinterpret_cast<FlushCallback>(fn)(*static_cast<const DrawTimeFacts*>(facts));
        }});
}

void geometry_set_rm_near_callback(RmNearCallback value) noexcept
{
    rsf_anomaly_geometry_set_near({reinterpret_cast<rsf_anomaly_geometry_fn>(value), 0,
        [](rsf_anomaly_geometry_fn fn, std::uintptr_t, const void* self) noexcept {
            reinterpret_cast<RmNearCallback>(fn)(const_cast<void*>(self));
        }});
}

void geometry_set_sink(GeometrySink value) noexcept
{
    rsf_anomaly_geometry_set_retired({reinterpret_cast<rsf_anomaly_geometry_fn>(value.retired),
        reinterpret_cast<std::uintptr_t>(value.context),
        [](rsf_anomaly_geometry_fn fn, std::uintptr_t user, const void* payload) noexcept {
            const auto& key = *static_cast<const rsf_anomaly_geometry_key*>(payload);
            reinterpret_cast<decltype(GeometrySink::retired)>(fn)(reinterpret_cast<void*>(user),
                {key.owner, key.visual, key.generation});
        }});
}

void geometry_set_draw_sink(DrawSink value) noexcept
{
    rsf_anomaly_geometry_set_draw({reinterpret_cast<rsf_anomaly_geometry_fn>(value.draw),
        reinterpret_cast<std::uintptr_t>(value.context),
        [](rsf_anomaly_geometry_fn fn, std::uintptr_t user, const void* payload) noexcept {
            reinterpret_cast<decltype(DrawSink::draw)>(fn)(reinterpret_cast<void*>(user),
                *static_cast<const DrawCapture*>(payload));
        }});
}

void geometry_enable_foliage_identity(bool value) noexcept { rsf_anomaly_geometry_set_foliage(value); }
bool geometry_foliage_identity_enabled() noexcept { return rsf_anomaly_geometry_foliage(); }

bool read_vertex_constants(std::uint32_t slot, std::uint32_t expected_size, std::uint32_t offset, void* out,
    std::uint32_t bytes) noexcept
{
    const auto source = vertex_constant_data(slot, expected_size, offset, bytes);
    return source && safe_copy(out, reinterpret_cast<const void*>(source), bytes);
}

bool write_vertex_constants(std::uint32_t slot, std::uint32_t expected_size, std::uint32_t offset, const void* in,
    std::uint32_t bytes) noexcept
{
    const auto target = vertex_constant_data(slot, expected_size, offset, bytes);
    return target && safe_copy(reinterpret_cast<void*>(target), in, bytes);
}

bool read_world_matrix(float (&out)[16]) noexcept
{
    return read(world(rsf_anomaly_geometry_binding()), out);
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
    for (std::size_t i = 0; i < sites.size(); ++i) {
        const auto& site = build.geometry->geometry_sites[i];
        sites[i] = {site.name, site.rva, site.expected, replacements[i], &originals[i]};
    }
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
void geometry_test_bind_module(std::byte* module, const BuildProfile& build) noexcept
{
    rsf_anomaly_geometry_test_bind(bind(module, build));
}
#endif
}
