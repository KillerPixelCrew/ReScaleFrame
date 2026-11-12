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
#include <string_view>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "%d: %s\n", __LINE__, #x); std::exit(1); } } while (false)
using namespace rsf::anomaly;
using namespace rsf::anomaly::geometry;
namespace {
using Create = void*(__fastcall*)(void*, const char*, void*);
using Duplicate = void*(__fastcall*)(void*, void*);
using Delete = void(__fastcall*)(void*, void**, std::int32_t);
using Render = void(__fastcall*)(void*, float);
std::array<std::byte, 512> visual{}, child{}, duplicate_visual{};
std::vector<temporal::InstanceKey> retirements;
void* expected_owner{};
const char* expected_name = "geometry-fixture.ogf";
void* expected_reader = &child;
bool retiring{};
std::uint64_t observed_generation{};
bool constructing{};
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
    CHECK(instance == child.data() && lod == 0.375f);
    const auto identity = borrow_draw();
    if (constructing) { CHECK(!identity); return; }
    CHECK(identity && identity->key.owner == reinterpret_cast<std::uintptr_t>(expected_owner));
    CHECK(identity->key.visual == reinterpret_cast<std::uintptr_t>(child.data()));
    CHECK(identity->domain == temporal::ProjectionDomain::FirstPerson && identity->view == temporal::ViewId::Scope);
    const auto capture = capture_current_draw();
    CHECK(capture && capture->world && capture->world->at(0, 3) == 123);
    observed_generation = identity->key.generation;
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
void retired(void*, temporal::InstanceKey key) noexcept
{
    retiring = true;
    retirements.push_back(key);
    retiring = false;
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
    CHECK(sites.size() >= 8);
    auto base = reinterpret_cast<std::byte*>(GetModuleHandleW(nullptr));
    const auto dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    const auto pe = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    const std::array<void*, 6> fixtures{reinterpret_cast<void*>(&create_fixture), reinterpret_cast<void*>(&duplicate_fixture),
        reinterpret_cast<void*>(&delete_fixture), reinterpret_cast<void*>(&render_fixture),
        reinterpret_cast<void*>(&skin_st_fixture), reinterpret_cast<void*>(&skin_pm_fixture)};
    const std::array<std::size_t, 6> indexes{0, 2, 3, 4, 6, 7};
    std::array<HookSite, 6> redirects;
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
    // A hierarchy that cannot be walked latches capture off instead of publishing a partial lifetime.
    const auto before_fault = geometry_stats();
    child_list[0] = nullptr;
    CHECK(create(&owner, expected_name, expected_reader) == visual.data());
    CHECK(geometry_stats().capture_faults == before_fault.capture_faults + 1 && !geometry_hooks_active());
    CHECK(create(&owner, expected_name, expected_reader) == visual.data());
    CHECK(geometry_stats().models_registered == before_fault.models_registered);
    child_list[0] = child.data();
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
    std::puts("PASS: actual x64 lifecycle/visual detours, RCX/RDX/R8/XMM1, hierarchy retirement before pooling/reuse, current world, full guarded production install/restore/reenable");
}
