// SPDX-License-Identifier: GPL-3.0-only
#include "native_geometry.h"
#include "geometry_profile.h"
#include "native_hooks.h"
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <array>
#include <cstring>
#include <cstdio>
#include <cstdlib>
#include <limits>
#include <string>
#include <string_view>
#include <vector>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "%d: %s\n", __LINE__, #x); std::exit(1); } } while (false)
using namespace rsf::anomaly;
using namespace rsf::anomaly::geometry;
namespace {
using Create = void*(__fastcall*)(void*, const char*, void*);
using Duplicate = void*(__fastcall*)(void*, void*);
using Delete = void(__fastcall*)(void*, void**, std::int32_t);
using Render = void(__fastcall*)(void*, float);
using Flush = void(__fastcall*)();
using Unary = void(__fastcall*)(void*);
std::array<std::byte, 512> visual{}, child{}, duplicate_visual{}, unregistered{};
std::vector<temporal::InstanceKey> retirements;
void* expected_owner{};
const char* expected_name = "geometry-fixture.ogf";
void* expected_reader = &child;
bool retiring{};
std::uint64_t observed_generation{};
bool constructing{};
// What the next render fixture invocation expects. The fixture stands in for the engine's Fvisual::Render, which
// reaches the D3D draw through CBackend::constants.flush.
void* expected_instance = child.data();
bool expect_identity = true;
bool call_flush = false;
std::optional<temporal::ViewId> expected_view = temporal::ViewId::Scope;
std::optional<temporal::ProjectionDomain> expected_domain = temporal::ProjectionDomain::FirstPerson;
Flush flush_target{};
Unary near_target{}, normal_target{};
__declspec(noinline) void* __fastcall duplicate_fixture(void*, void*);
__declspec(noinline) void __fastcall render_fixture(void*, float);
__declspec(noinline) void* __fastcall create_fixture(void* owner, const char* name, void* reader)
{
    CHECK(owner == expected_owner && name == expected_name && reader == expected_reader);
    constructing = true;
    CHECK(duplicate_fixture(owner, visual.data()) == child.data());
    render_fixture(child.data(), 0.375f); // No identity before the enclosing model is returned.
    constructing = false;
    return visual.data();
}
__declspec(noinline) void* __fastcall duplicate_fixture(void* owner, void* source)
{
    CHECK(owner == expected_owner && source == visual.data());
    return constructing ? child.data() : duplicate_visual.data();
}
__declspec(noinline) void __fastcall delete_fixture(void* owner, void** pointer, std::int32_t discard)
{
    CHECK(owner == expected_owner && *pointer == visual.data() && discard == 0x12345678);
    CHECK(retirements.size() >= 2 && !retiring);
    *pointer = nullptr;
}
__declspec(noinline) void __fastcall render_fixture(void* instance, float lod)
{
    CHECK(instance == expected_instance && lod == 0.375f);
    const auto identity = borrow_draw();
    if (constructing) { CHECK(!identity); return; }
    if (!expect_identity) {
        CHECK(!identity);
    } else {
        CHECK(identity && identity->key.owner == reinterpret_cast<std::uintptr_t>(expected_owner));
        CHECK(identity->key.visual == reinterpret_cast<std::uintptr_t>(child.data()));
        CHECK(identity->domain == expected_domain && identity->view == expected_view);
        const auto capture = capture_current_draw();
        CHECK(capture && capture->world && capture->world->at(0, 3) == 123);
        observed_generation = identity->key.generation;
    }
    if (call_flush) flush_target();
}
__declspec(noinline) void __fastcall skin_st_fixture(void* instance, float lod)
{
    CHECK(instance == child.data() && lod == 0.625f);
    CHECK(borrow_draw() && borrow_draw()->kind == DrawKind::HardwareSkinned);
    const auto capture = capture_current_draw();
    CHECK(capture && capture->bones.size() == 1 && capture->bones[0].at(0, 3) == 456);
}
__declspec(noinline) void __fastcall skin_pm_fixture(void* instance, float lod)
{
    CHECK(instance == child.data() && lod == 0.875f);
    CHECK(borrow_draw() && borrow_draw()->kind == DrawKind::HardwareSkinned);
    const auto capture = capture_current_draw();
    CHECK(capture && capture->bones.size() == 1 && capture->bones[0].at(0, 3) == 789);
}
std::uint32_t flush_calls{};
__declspec(noinline) void __fastcall flush_fixture() { ++flush_calls; if (flush_calls == 0xffffffffu) std::abort(); }
// Foliage fixtures: a tree visual and CDetailManager::Render, both reaching the D3D draw through the flush.
std::array<std::byte, 512> tree_visual{};
__declspec(noinline) void __fastcall tree_st_fixture(void* instance, float lod)
{
    CHECK(instance == tree_visual.data() && lod == 0.5f);
    flush_target();
}
__declspec(noinline) void __fastcall tree_pm_fixture(void* instance, float lod)
{
    CHECK(instance == tree_visual.data() && lod == 0.25f);
    flush_target();
}
std::uint32_t detail_calls{};
bool flush_inside_detail{};
__declspec(noinline) void __fastcall detail_fixture(void* self)
{
    CHECK(self == &detail_calls);
    ++detail_calls;
    flush_target();
    flush_target();
}
std::vector<DrawTimeFacts> seen_facts;
void collect_facts(const DrawTimeFacts& facts) noexcept
{
    DrawTimeFacts copy = facts;
    copy.capture = nullptr;
    seen_facts.push_back(copy);
}
struct Context {
    std::optional<temporal::ViewId> view;
    std::optional<temporal::ProjectionDomain> domain;
    bool operator==(const Context&) const = default;
};
// Observes the draw context a visual drawn right now would receive.
Context context_now()
{
    static IdentityRegistry registry;
    static const auto key = registry.acquire(1, 2);
    DrawScope probe(registry, key, DrawKind::Rigid, nullptr);
    const auto identity = borrow_draw();
    CHECK(identity);
    return {identity->view, identity->domain};
}
Context hud_context, sorted_before, sorted_hud, sorted_after, emissive_before, emissive_hud, emissive_after;
std::uint32_t hud_calls{}, near_calls{}, normal_calls{};
__declspec(noinline) void __fastcall hud_fixture(void* self)
{
    CHECK(self == &hud_calls);
    hud_context = context_now();
    ++hud_calls;
}
__declspec(noinline) void __fastcall near_fixture(void* self) { CHECK(self == &hud_calls); ++near_calls; if (!near_calls) std::abort(); }
__declspec(noinline) void __fastcall normal_fixture(void* self) { CHECK(self == &hud_calls); ++normal_calls; if (!normal_calls) std::abort(); }
__declspec(noinline) void __fastcall sorted_fixture(void* self)
{
    CHECK(self == &hud_calls);
    sorted_before = context_now();
    near_target(self);
    sorted_hud = context_now();
    normal_target(self);
    sorted_after = context_now();
}
__declspec(noinline) void __fastcall emissive_fixture(void* self)
{
    CHECK(self == &hud_calls);
    emissive_before = context_now();
    near_target(self);
    emissive_hud = context_now();
    // A function that leaves without rmNormal must still close the window before the enclosing scopes unwind.
}
void retired(void*, temporal::InstanceKey key) noexcept
{
    retiring = true;
    retirements.push_back(key);
    retiring = false;
}
void collect_draw(void* context, const DrawCapture& capture) noexcept
{
    static_cast<std::vector<DrawCapture>*>(context)->push_back(capture);
}
void collect_line(void* context, const char* line)
{
    static_cast<std::vector<std::string>*>(context)->emplace_back(line);
}
std::uint64_t cell(const GeometryCoverage& c, DrawPass pass, VisualType type, DrawVerdict verdict)
{
    return c.draws[static_cast<std::size_t>(pass)][static_cast<std::size_t>(type)][static_cast<std::size_t>(verdict)];
}
}
int main()
{
    std::vector<std::byte> module(native_image_size);
    CHECK(!install_native_geometry(nullptr, native_image_size, dx11_hash));
    CHECK(!install_native_geometry(module.data(), native_image_size, avx_hash));
    CHECK(!install_native_geometry(module.data(), native_image_size, dx11_hash));
    CHECK(!geometry_hooks_active());
    geometry_test_bind_module(module.data());
    geometry_set_sink({nullptr, &retired});
    const float world[16]{1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 123, 0, 0, 1};
    const float identity_world[16]{1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
    std::memcpy(module.data() + world_matrix_rva, world, sizeof(world));
    const void* table = module.data() + hierarchy_vtables[0];
    std::memcpy(visual.data(), &table, sizeof(table));
    void* child_list[]{child.data()};
    const void* begin = child_list;
    const void* end = child_list + 1;
    std::memcpy(visual.data() + 96, &begin, sizeof(begin));
    std::memcpy(visual.data() + 104, &end, sizeof(end));
    std::memcpy(visual.data() + 112, &end, sizeof(end));
    int owner{};
    expected_owner = &owner;
    const auto sites = native_geometry_hook_sites();
    CHECK(sites.size() == geometry_sites.size() && sites.size() == 17);
    auto base = reinterpret_cast<std::byte*>(GetModuleHandleW(nullptr));
    const auto dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    const auto pe = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    const std::array<void*, 15> fixtures{reinterpret_cast<void*>(&create_fixture), reinterpret_cast<void*>(&duplicate_fixture),
        reinterpret_cast<void*>(&delete_fixture), reinterpret_cast<void*>(&render_fixture),
        reinterpret_cast<void*>(&skin_st_fixture), reinterpret_cast<void*>(&skin_pm_fixture),
        reinterpret_cast<void*>(&flush_fixture), reinterpret_cast<void*>(&hud_fixture),
        reinterpret_cast<void*>(&sorted_fixture), reinterpret_cast<void*>(&emissive_fixture),
        reinterpret_cast<void*>(&near_fixture), reinterpret_cast<void*>(&normal_fixture),
        reinterpret_cast<void*>(&tree_st_fixture), reinterpret_cast<void*>(&tree_pm_fixture),
        reinterpret_cast<void*>(&detail_fixture)};
    const std::array<std::size_t, 15> indexes{0, 2, 3, 4, 6, 7, geometry_site_flush, geometry_site_render_hud,
        geometry_site_render_sorted, geometry_site_render_emissive, geometry_site_rm_near, geometry_site_rm_normal,
        8, 9, geometry_site_detail_render};
    std::array<HookSite, 15> redirects;
    for (std::size_t i = 0; i < redirects.size(); ++i) {
        redirects[i] = sites[indexes[i]];
        redirects[i].rva = static_cast<std::uint32_t>(reinterpret_cast<std::byte*>(fixtures[i]) - base);
        std::memcpy(redirects[i].expected.data(), fixtures[i], 16);
    }
    HookSet hooks;
    CHECK(hooks.install(base, pe->OptionalHeader.SizeOfImage, redirects));
    const auto create = reinterpret_cast<Create>(fixtures[0]);
    const auto duplicate = reinterpret_cast<Duplicate>(fixtures[1]);
    const auto destroy = reinterpret_cast<Delete>(fixtures[2]);
    const auto render = reinterpret_cast<Render>(fixtures[3]);
    flush_target = reinterpret_cast<Flush>(fixtures[6]);
    const auto hud = reinterpret_cast<Unary>(fixtures[7]);
    const auto sorted = reinterpret_cast<Unary>(fixtures[8]);
    const auto emissive = reinterpret_cast<Unary>(fixtures[9]);
    near_target = reinterpret_cast<Unary>(fixtures[10]);
    normal_target = reinterpret_cast<Unary>(fixtures[11]);
    const auto tree_st = reinterpret_cast<Render>(fixtures[12]);
    const auto tree_pm = reinterpret_cast<Render>(fixtures[13]);
    const auto detail_render = reinterpret_cast<Unary>(fixtures[14]);
    CHECK(create(&owner, expected_name, expected_reader) == visual.data());
    CHECK(duplicate(&owner, visual.data()) == duplicate_visual.data());
    {
        DrawContextScope context(temporal::ViewId::Scope, temporal::ProjectionDomain::FirstPerson);
        render(child.data(), 0.375f);
    }
    const auto first_generation = observed_generation;
    std::array<std::byte, 256> parent{};
    std::array<std::byte, 168> bone{};
    const auto parent_pointer = parent.data();
    const auto bone_pointer = bone.data();
    std::memcpy(parent.data() + 208, &bone_pointer, sizeof(bone_pointer));
    const auto check_skin = [&](std::size_t index, std::size_t offset, float lod, float translation) {
        const std::uint16_t mode = 2;
        const std::uint32_t count = 1;
        std::memcpy(child.data() + offset + 8, &parent_pointer, sizeof(parent_pointer));
        std::memcpy(child.data() + offset + 56, &mode, sizeof(mode));
        std::memcpy(child.data() + offset + 60, &count, sizeof(count));
        float transform[16];
        std::memcpy(transform, world, sizeof(transform));
        transform[12] = translation;
        std::memcpy(bone.data() + 64, transform, sizeof(transform));
        reinterpret_cast<Render>(fixtures[index])(child.data(), lod);
    };
    check_skin(4, 160, 0.625f, 456);
    check_skin(5, 208, 0.875f, 789);
    void* pointer = visual.data();
    destroy(&owner, &pointer, 0x12345678);
    CHECK(!pointer && !borrow_draw());
    CHECK(create(&owner, expected_name, expected_reader) == visual.data());
    {
        DrawContextScope context(temporal::ViewId::Scope, temporal::ProjectionDomain::FirstPerson);
        render(child.data(), 0.375f);
    }
    CHECK(observed_generation > first_generation);

    // Draw-time call site: the hooked CBackend::constants.flush reaches capture_current_draw for an identified
    // main-pass draw and hands the copy to the sink. The registered visual stays child.data().
    std::vector<DrawCapture> sunk;
    geometry_set_draw_sink({&sunk, &collect_draw});
    call_flush = true;
    {
        const auto before = geometry_coverage();
        const auto calls_before = flush_calls;
        DrawContextScope context(temporal::ViewId::Scope, temporal::ProjectionDomain::FirstPerson);
        render(child.data(), 0.375f);
        const auto after = geometry_coverage();
        CHECK(flush_calls == calls_before + 1); // The original flush still runs.
        CHECK(sunk.size() == 1 && sunk[0].key.visual == reinterpret_cast<std::uintptr_t>(child.data()));
        CHECK(sunk[0].view == temporal::ViewId::Scope && sunk[0].domain == temporal::ProjectionDomain::FirstPerson);
        CHECK(sunk[0].world && sunk[0].world->at(0, 3) == 123);
        CHECK(cell(after, DrawPass::Main, VisualType::Static, DrawVerdict::Identified) ==
            cell(before, DrawPass::Main, VisualType::Static, DrawVerdict::Identified) + 1);
        CHECK(after.flush_total[0] == before.flush_total[0] + 1 && after.flush_in_visual[0] == before.flush_in_visual[0] + 1);
        CHECK(after.captured == before.captured + 1 && after.sunk == before.sunk + 1 && after.capture_no_world == before.capture_no_world);
    }
    // A flush outside every visual is counted as such and captures nothing.
    {
        const auto before = geometry_coverage();
        flush_target();
        const auto after = geometry_coverage();
        CHECK(after.flush_total[0] == before.flush_total[0] + 1 && after.flush_in_visual[0] == before.flush_in_visual[0]);
        CHECK(after.captured == before.captured && sunk.size() == 1);
    }
    // An identified draw with no view/projection context is still captured; the missing context is counted.
    {
        expected_view.reset();
        expected_domain.reset();
        const auto before = geometry_coverage();
        sunk.clear();
        render(child.data(), 0.375f);
        const auto after = geometry_coverage();
        CHECK(sunk.size() == 1 && !sunk[0].view && !sunk[0].domain);
        CHECK(after.capture_no_context == before.capture_no_context + 1);
        expected_view = temporal::ViewId::Scope;
        expected_domain = temporal::ProjectionDomain::FirstPerson;
    }
    // Shadow-map and Z-fill phase: nothing is captured, and the counters say why.
    {
        const std::uint32_t smap = phase_smap;
        std::memcpy(module.data() + render_phase_rva, &smap, sizeof(smap));
        const auto before = geometry_coverage();
        sunk.clear();
        {
            DrawContextScope context(temporal::ViewId::Scope, temporal::ProjectionDomain::FirstPerson);
            render(child.data(), 0.375f);
        }
        const auto after = geometry_coverage();
        CHECK(sunk.empty() && after.captured == before.captured);
        CHECK(cell(after, DrawPass::Shadow, VisualType::Static, DrawVerdict::IgnoredShadow) ==
            cell(before, DrawPass::Shadow, VisualType::Static, DrawVerdict::IgnoredShadow) + 1);
        CHECK(cell(after, DrawPass::Main, VisualType::Static, DrawVerdict::Identified) ==
            cell(before, DrawPass::Main, VisualType::Static, DrawVerdict::Identified));
        CHECK(after.flush_total[1] == before.flush_total[1] + 1 && after.flush_in_visual[1] == before.flush_in_visual[1] + 1);
        const std::uint32_t other_phase = 7;
        std::memcpy(module.data() + render_phase_rva, &other_phase, sizeof(other_phase));
        {
            DrawContextScope context(temporal::ViewId::Scope, temporal::ProjectionDomain::FirstPerson);
            render(child.data(), 0.375f);
        }
        const auto unknown = geometry_coverage();
        CHECK(cell(unknown, DrawPass::Unknown, VisualType::Static, DrawVerdict::Identified) ==
            cell(after, DrawPass::Unknown, VisualType::Static, DrawVerdict::Identified) + 1);
        CHECK(sunk.empty() && unknown.captured == before.captured);
        const std::uint32_t normal_phase = phase_normal;
        std::memcpy(module.data() + render_phase_rva, &normal_phase, sizeof(normal_phase));
    }
    // Unregistered rigid visuals: level geometry (exactly Fidentity) needs no object motion; anything else is a gap.
    expect_identity = false;
    expected_instance = unregistered.data();
    {
        std::memcpy(module.data() + world_matrix_rva, identity_world, sizeof(identity_world));
        const auto before = geometry_coverage();
        sunk.clear();
        render(unregistered.data(), 0.375f);
        const auto after = geometry_coverage();
        CHECK(cell(after, DrawPass::Main, VisualType::Static, DrawVerdict::IgnoredStaticWorld) ==
            cell(before, DrawPass::Main, VisualType::Static, DrawVerdict::IgnoredStaticWorld) + 1);
        CHECK(sunk.empty() && after.captured == before.captured);
        CHECK(after.flush_in_visual[0] == before.flush_in_visual[0] + 1);

        std::memcpy(module.data() + world_matrix_rva, world, sizeof(world));
        render(unregistered.data(), 0.375f);
        const auto moving = geometry_coverage();
        CHECK(cell(moving, DrawPass::Main, VisualType::Static, DrawVerdict::UnidentifiedDynamic) ==
            cell(after, DrawPass::Main, VisualType::Static, DrawVerdict::UnidentifiedDynamic) + 1);
        CHECK(sunk.empty() && moving.captured == before.captured);

        const std::uint32_t smap = phase_smap;
        std::memcpy(module.data() + render_phase_rva, &smap, sizeof(smap));
        render(unregistered.data(), 0.375f);
        const std::uint32_t normal_phase = phase_normal;
        std::memcpy(module.data() + render_phase_rva, &normal_phase, sizeof(normal_phase));
        const auto shadow = geometry_coverage();
        CHECK(cell(shadow, DrawPass::Shadow, VisualType::Static, DrawVerdict::IgnoredShadow) ==
            cell(moving, DrawPass::Shadow, VisualType::Static, DrawVerdict::IgnoredShadow) + 1);
    }
    // The same visual live under two owners cannot be named, and says so instead of guessing.
    {
        int other_owner{};
        expected_owner = &other_owner;
        expected_instance = child.data(); // The factory fixture draws the child once while constructing.
        CHECK(create(&other_owner, expected_name, expected_reader) == visual.data());
        expected_owner = &owner;
        const auto before = geometry_coverage();
        sunk.clear();
        render(child.data(), 0.375f);
        const auto after = geometry_coverage();
        CHECK(cell(after, DrawPass::Main, VisualType::Static, DrawVerdict::UnidentifiedAmbiguous) ==
            cell(before, DrawPass::Main, VisualType::Static, DrawVerdict::UnidentifiedAmbiguous) + 1);
        CHECK(sunk.empty());
        void* second = visual.data();
        expected_owner = &other_owner;
        destroy(&other_owner, &second, 0x12345678);
        expected_owner = &owner;
        CHECK(!second);
        expect_identity = true;
        DrawContextScope context(temporal::ViewId::Scope, temporal::ProjectionDomain::FirstPerson);
        render(child.data(), 0.375f);
        CHECK(sunk.size() == 1);
    }
    call_flush = false;
    geometry_set_draw_sink({});

    // Foliage. A level tree never enters the model registry. Without foliage identity it is the measured gap
    // (unidentified_deformed); with it the tree is named by its address, nothing is captured, and the flush facts carry
    // the visual, its type and the progressive tree's slide window.
    {
        expect_identity = false;
        geometry_set_flush_callback(&collect_facts);
        const std::uint32_t lod5 = 5;
        std::memcpy(tree_visual.data() + tree_pm_last_lod_offset, &lod5, sizeof(lod5));
        CHECK(!geometry_foliage_identity_enabled());
        auto before = geometry_coverage();
        seen_facts.clear();
        tree_st(tree_visual.data(), 0.5f);
        auto after = geometry_coverage();
        CHECK(cell(after, DrawPass::Main, VisualType::TreeST, DrawVerdict::UnidentifiedDeformed) ==
            cell(before, DrawPass::Main, VisualType::TreeST, DrawVerdict::UnidentifiedDeformed) + 1);
        CHECK(cell(after, DrawPass::Main, VisualType::TreeST, DrawVerdict::IdentifiedFoliage) ==
            cell(before, DrawPass::Main, VisualType::TreeST, DrawVerdict::IdentifiedFoliage));

        geometry_enable_foliage_identity(true);
        CHECK(geometry_foliage_identity_enabled());
        before = geometry_coverage();
        const auto stats_before = geometry_stats();
        seen_facts.clear();
        tree_st(tree_visual.data(), 0.5f);
        tree_pm(tree_visual.data(), 0.25f);
        after = geometry_coverage();
        CHECK(cell(after, DrawPass::Main, VisualType::TreeST, DrawVerdict::IdentifiedFoliage) ==
            cell(before, DrawPass::Main, VisualType::TreeST, DrawVerdict::IdentifiedFoliage) + 1);
        CHECK(cell(after, DrawPass::Main, VisualType::TreePM, DrawVerdict::IdentifiedFoliage) ==
            cell(before, DrawPass::Main, VisualType::TreePM, DrawVerdict::IdentifiedFoliage) + 1);
        CHECK(cell(after, DrawPass::Main, VisualType::TreeST, DrawVerdict::UnidentifiedDeformed) ==
            cell(before, DrawPass::Main, VisualType::TreeST, DrawVerdict::UnidentifiedDeformed));
        CHECK(geometry_stats().draws_identified == stats_before.draws_identified + 2);
        CHECK(after.captured == before.captured);   // a foliage identity is not a registry capture
        CHECK(seen_facts.size() == 2);
        const auto address = reinterpret_cast<std::uintptr_t>(tree_visual.data());
        CHECK(seen_facts[0].in_visual && seen_facts[0].kind == DrawKind::Tree && seen_facts[0].visual == address &&
            seen_facts[0].type == VisualType::TreeST && seen_facts[0].tree_lod == -1 && !seen_facts[0].detail &&
            seen_facts[0].classification.verdict == DrawVerdict::IdentifiedFoliage && !seen_facts[0].capture);
        CHECK(seen_facts[1].visual == address && seen_facts[1].type == VisualType::TreePM && seen_facts[1].tree_lod == 5);
        // A shadow-map tree needs nothing.
        const std::uint32_t smap = phase_smap;
        std::memcpy(module.data() + render_phase_rva, &smap, sizeof(smap));
        before = geometry_coverage();
        tree_st(tree_visual.data(), 0.5f);
        after = geometry_coverage();
        CHECK(cell(after, DrawPass::Shadow, VisualType::TreeST, DrawVerdict::IgnoredShadow) ==
            cell(before, DrawPass::Shadow, VisualType::TreeST, DrawVerdict::IgnoredShadow) + 1);
        const std::uint32_t normal_phase = phase_normal;
        std::memcpy(module.data() + render_phase_rva, &normal_phase, sizeof(normal_phase));

        // CDetailManager::Render: every flush inside it is a detail draw in its pass; outside it none is.
        before = geometry_coverage();
        seen_facts.clear();
        detail_render(&detail_calls);
        flush_target();
        after = geometry_coverage();
        CHECK(seen_facts.size() == 3 && seen_facts[0].detail && seen_facts[1].detail && !seen_facts[2].detail);
        CHECK(!seen_facts[0].in_visual && seen_facts[0].pass == DrawPass::Main);
        CHECK(after.detail_renders[0] == before.detail_renders[0] + 1 && after.flush_in_detail[0] == before.flush_in_detail[0] + 2);
        std::memcpy(module.data() + render_phase_rva, &smap, sizeof(smap));
        detail_render(&detail_calls);
        std::memcpy(module.data() + render_phase_rva, &normal_phase, sizeof(normal_phase));
        const auto shadow = geometry_coverage();
        CHECK(shadow.detail_renders[1] == after.detail_renders[1] + 1 && shadow.flush_in_detail[1] == after.flush_in_detail[1] + 2);
        geometry_set_flush_callback(nullptr);
        geometry_enable_foliage_identity(false);
        expect_identity = true;
    }
    // The engine's staged vertex constants: CBackend::m_aVertexConstants[slot] -> dx10ConstantBuffer { size +0x58,
    // data +0x60 }. Reads and writes are refused unless the object reports the reflected size.
    {
        std::array<std::byte, 0x80> object{};
        std::array<float, 64> staged{};
        for (std::size_t i = 0; i < staged.size(); ++i) staged[i] = static_cast<float>(i + 1);
        const std::uint32_t size = sizeof(staged);
        const auto data = reinterpret_cast<std::uintptr_t>(staged.data());
        std::memcpy(object.data() + constant_buffer_size_offset, &size, sizeof(size));
        std::memcpy(object.data() + constant_buffer_data_offset, &data, sizeof(data));
        const auto object_pointer = reinterpret_cast<std::uintptr_t>(object.data());
        std::memcpy(module.data() + vertex_constant_buffers_rva + 3 * sizeof(object_pointer), &object_pointer, sizeof(object_pointer));
        float out[4]{};
        CHECK(read_vertex_constants(3, size, 16, out, sizeof(out)) && out[0] == 5.0f && out[3] == 8.0f);
        CHECK(!read_vertex_constants(3, size + 16, 16, out, sizeof(out)));          // reflected size differs
        CHECK(!read_vertex_constants(3, size, size - 8, out, sizeof(out)));         // runs past the buffer
        CHECK(!read_vertex_constants(constant_buffer_slots, size, 0, out, sizeof(out)));
        CHECK(!read_vertex_constants(4, size, 0, out, sizeof(out)));                // no buffer in that slot
        const float poison[4]{-1, -2, -3, -4};
        CHECK(write_vertex_constants(3, size, 32, poison, sizeof(poison)) && staged[8] == -1.0f && staged[11] == -4.0f &&
            staged[7] == 8.0f && staged[12] == 13.0f);
        CHECK(!write_vertex_constants(3, size + 4, 32, poison, sizeof(poison)) && staged[8] == -1.0f);
        float matrix[16]{};
        CHECK(read_world_matrix(matrix));
    }

    // Context producers: the engine's IsSVPFrame predicate selects the view, render_hud and the rmNear window the
    // first-person domain.
    const auto set_viewport = [&](bool active, std::uint8_t delay, std::uint32_t frame) {
        std::memcpy(module.data() + second_viewport_rva, &active, sizeof(active));
        std::memcpy(module.data() + second_viewport_rva + second_viewport_delay_offset, &delay, sizeof(delay));
        std::memcpy(module.data() + device_frame_rva, &frame, sizeof(frame));
    };
    CHECK(!current_render_view(nullptr));
    set_viewport(false, 2, 4);
    CHECK(current_render_view(module.data()) == temporal::ViewId::Main);
    set_viewport(true, 2, 3);
    CHECK(current_render_view(module.data()) == temporal::ViewId::Main);
    set_viewport(true, 2, 4);
    CHECK(current_render_view(module.data()) == temporal::ViewId::Scope);
    set_viewport(true, 0, 4);
    CHECK(current_render_view(module.data()) == temporal::ViewId::Main);
    const Context outside{};
    CHECK(context_now() == outside);
    {
        set_viewport(true, 2, 4);
        const RenderViewScope view;
        const Context world_scope{temporal::ViewId::Scope, temporal::ProjectionDomain::World};
        const Context hud_scope{temporal::ViewId::Scope, temporal::ProjectionDomain::FirstPerson};
        CHECK(context_now() == world_scope);
        hud(&hud_calls);
        CHECK(hud_calls == 1 && hud_context == hud_scope);
        CHECK(context_now() == world_scope);
        sorted(&hud_calls);
        CHECK(sorted_before == world_scope && sorted_hud == hud_scope && sorted_after == world_scope);
        emissive(&hud_calls);
        CHECK(emissive_before == world_scope && emissive_hud == hud_scope);
        CHECK(context_now() == world_scope); // The window closed on return although rmNormal never ran.
        CHECK(near_calls == 2 && normal_calls == 1);
        near_target(&hud_calls); // Outside sorted/emissive (particles, light volumes) rmNear opens nothing.
        CHECK(context_now() == world_scope);
        normal_target(&hud_calls);
    }
    CHECK(context_now() == outside);
    set_viewport(false, 2, 4);
    {
        const RenderViewScope view;
        CHECK((context_now() == Context{temporal::ViewId::Main, temporal::ProjectionDomain::World}));
    }
    {
        const auto coverage = geometry_coverage();
        CHECK(coverage.views_main >= 1 && coverage.views_scope >= 1 && coverage.hud_scopes >= 1 && coverage.near_windows == 2);
    }
    std::vector<std::string> lines;
    geometry_report_coverage(&collect_line, &lines);
    const auto has = [&](std::string_view text) {
        for (const auto& line : lines) if (line.find(text) != std::string::npos) return true;
        return false;
    };
    CHECK(has("coverage render-entries main=") && has("pass=main type=static identified=") &&
        has("pass=shadow type=static identified=") && has("pass=other type=static") &&
        has("draw-time flush main=") && has("captured=") && has("hud_scopes="));
    for (const auto& line : lines) CHECK(line.size() < 600);

    // A hierarchy that cannot be walked latches capture off instead of publishing a partial lifetime.
    const auto before_fault = geometry_stats();
    child_list[0] = nullptr;
    CHECK(create(&owner, expected_name, expected_reader) == visual.data());
    CHECK(geometry_stats().capture_faults == before_fault.capture_faults + 1 && !geometry_hooks_active());
    CHECK(create(&owner, expected_name, expected_reader) == visual.data());
    CHECK(geometry_stats().models_registered == before_fault.models_registered);
    child_list[0] = child.data();
    // With capture latched off a draw is not identified and says so; the flush hook stops observing.
    {
        const auto before = geometry_coverage();
        expect_identity = false;
        render(child.data(), 0.375f);
        const auto after = geometry_coverage();
        CHECK(cell(after, DrawPass::Main, VisualType::Static, DrawVerdict::UnidentifiedNoCapture) ==
            cell(before, DrawPass::Main, VisualType::Static, DrawVerdict::UnidentifiedNoCapture) + 1);
    }
    CHECK(hooks.disable());
    geometry_retire_all();
    geometry_set_sink({});
    const auto stats = geometry_stats();
    CHECK(stats.models_registered >= 5 && stats.models_retired >= 5 && stats.draws_identified >= 3 &&
        stats.draws_unidentified >= 1 && stats.capture_faults == 1);
    // Production installation path, using guarded code copied into an executable-memory fixture.
    // No game code is executed, no game process or installation is modified.
    auto image = static_cast<std::byte*>(VirtualAlloc(nullptr, native_image_size, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE));
    CHECK(image);
    auto fixture_dos = reinterpret_cast<IMAGE_DOS_HEADER*>(image);
    fixture_dos->e_magic = IMAGE_DOS_SIGNATURE;
    fixture_dos->e_lfanew = 0x80;
    auto fixture_pe = reinterpret_cast<IMAGE_NT_HEADERS64*>(image + fixture_dos->e_lfanew);
    fixture_pe->Signature = IMAGE_NT_SIGNATURE;
    fixture_pe->FileHeader.Machine = IMAGE_FILE_MACHINE_AMD64;
    fixture_pe->OptionalHeader.Magic = IMAGE_NT_OPTIONAL_HDR64_MAGIC;
    fixture_pe->OptionalHeader.SizeOfImage = native_image_size;
    for (const auto& site : geometry_sites) std::memcpy(image + site.rva, site.expected.data(), 16);
    for (const auto& site : geometry_layout_sites) std::memcpy(image + site.rva, site.expected.data(), 16);
    // Every detour and every layout guard refuses on a single changed byte, and nothing is written before the refusal.
    for (const auto& layout : geometry_layout_sites) {
        image[layout.rva] ^= std::byte{1};
        CHECK(!install_native_geometry(image, native_image_size, dx11_hash));
        image[layout.rva] ^= std::byte{1};
    }
    for (const auto& site : geometry_sites) {
        image[site.rva + 5] ^= std::byte{0x80};
        CHECK(!install_native_geometry(image, native_image_size, dx11_hash));
        image[site.rva + 5] ^= std::byte{0x80};
    }
    const auto corrupted = geometry_sites.back();
    image[corrupted.rva] ^= std::byte{1};
    CHECK(!install_native_geometry(image, native_image_size, dx11_hash));
    for (std::size_t i = 0; i + 1 < geometry_sites.size(); ++i)
        CHECK(!std::memcmp(image + geometry_sites[i].rva, geometry_sites[i].expected.data(), 16));
    image[corrupted.rva] ^= std::byte{1};
    CHECK(install_native_geometry(image, native_image_size, dx11_hash));
    CHECK(geometry_hooks_active());
    CHECK(disable_native_geometry() && !geometry_hooks_active());
    for (const auto& site : geometry_sites) CHECK(!std::memcmp(image + site.rva, site.expected.data(), 16));
    CHECK(!install_native_geometry(image, native_image_size, avx_hash));
    CHECK(install_native_geometry(image, native_image_size, dx11_hash));
    CHECK(disable_native_geometry());
    // Executable fixture allocations and trampolines intentionally live until process exit.
    std::puts("PASS: actual x64 lifecycle/visual detours, RCX/RDX/R8/XMM1, hierarchy retirement before pooling/reuse, current world, "
        "draw-time flush capture and sink, pass/identity/static-world classification, view and first-person context producers, "
        "per-category coverage, full guarded production install/restore/reenable");
}
