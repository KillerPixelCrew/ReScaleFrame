// SPDX-License-Identifier: GPL-3.0-only
// The geometry producer's CBackend::constants.flush detour (redirected onto a fixture function in this executable)
// calls the shared flush slot (geometry_set_flush_callback) AFTER the original flush, with the draw facts the motion
// stage keys on: outside any scope, inside a context scope only, and inside a visual's draw scope. No game code runs.
#include "geometry_profile.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include <windows.h>
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <string>
#include <vector>

using namespace rsf::anomaly;
using namespace rsf::anomaly::geometry;

namespace {
int checks = 0;
void require(bool ok, const char* message)
{
    ++checks;
    if (!ok) throw std::runtime_error(message);
}
std::vector<std::string> order;
__declspec(noinline) void __fastcall flush_fixture()
{
    order.emplace_back("flush");
    if (order.size() > 1000) std::abort();
}
DrawTimeFacts last{};
int callbacks = 0;
void callback(const DrawTimeFacts& facts) noexcept
{
    try { order.emplace_back("callback"); } catch (...) {}
    last = facts;
    ++callbacks;
}
}

int main()
{
    try {
        auto* base = reinterpret_cast<std::byte*>(GetModuleHandleW(nullptr));
        const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
        const auto* pe = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
        HookSite redirect = native_geometry_hook_sites(standard_build)[geometry_site_flush];   // only the replacement is used
        redirect.rva = static_cast<std::uint32_t>(reinterpret_cast<std::byte*>(&flush_fixture) - base);
        std::memcpy(redirect.expected.data(), reinterpret_cast<const void*>(&flush_fixture), 16);
        HookSet hooks;
        require(hooks.install(base, pe->OptionalHeader.SizeOfImage, {&redirect, 1}), "redirected flush detour installs");

        flush_fixture();
        require(order == std::vector<std::string>{"flush"} && callbacks == 0, "no slot: only the original runs");
        geometry_set_flush_callback(&callback);
        order.clear();
        flush_fixture();
        require(order == std::vector<std::string>{"flush", "callback"}, "slot runs after the original flush");
        require(!last.in_visual && !last.view && !last.domain && !last.capture, "outside every scope");
        {
            const DrawContextScope context(temporal::ViewId::Scope, temporal::ProjectionDomain::World);
            flush_fixture();
            require(!last.in_visual && last.view == temporal::ViewId::Scope && last.domain == temporal::ProjectionDomain::World,
                "context scope supplies view/domain for draws outside a visual (details, LODs)");
            const DrawContextScope hud(DrawContextScope::InheritView{}, temporal::ProjectionDomain::FirstPerson);
            IdentityRegistry registry;
            const DrawScope scope(registry, std::nullopt, DrawKind::HardwareSkinned, nullptr, nullptr,
                {DrawPass::Main, VisualType::SkeletonST, DrawVerdict::UnidentifiedDynamic});
            flush_fixture();
            require(last.in_visual && last.kind == DrawKind::HardwareSkinned &&
                last.classification.verdict == DrawVerdict::UnidentifiedDynamic && last.classification.type == VisualType::SkeletonST,
                "visual scope facts");
            require(last.view == temporal::ViewId::Scope && last.domain == temporal::ProjectionDomain::FirstPerson,
                "first-person domain inherits the view");
            require(!last.capture, "an unidentified draw has no capture");
        }
        geometry_set_flush_callback(nullptr);
        const int before = callbacks;
        flush_fixture();
        require(callbacks == before, "cleared slot");
        geometry_set_flush_callback(&callback);
        require(hooks.disable(), "disable");
        order.clear();
        flush_fixture();
        require(order == std::vector<std::string>{"flush"}, "disabled detour: original only");
        geometry_set_flush_callback(nullptr);
        std::printf("anomaly motion hooks: %d checks passed\n", checks);
        return 0;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "FAILED %s\n", error.what());
        return 1;
    }
}
