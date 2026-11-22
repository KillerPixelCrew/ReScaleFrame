// SPDX-License-Identifier: MIT
// Native launcher for the Rust fixture. The real CBackend::constants.flush
// detour targets this executable's local function, with the original hook ABI.
#include "geometry_profile.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include <windows.h>
#include <cstdint>
#include <cstdlib>
#include <cstring>

using namespace rsf::anomaly;
using namespace rsf::anomaly::geometry;

namespace {
struct Facts {
    uint8_t in_visual, view, domain, capture, kind, verdict, visual_type;
};
using Sink = void (*)(const Facts*) noexcept;
static_assert(sizeof(Facts) == 7);

Sink sink{};
unsigned original_calls{};
rsf_anomaly_hook_set hooks{};

__declspec(noinline) void __fastcall flush_fixture()
{
    sink(nullptr);
    if (++original_calls > 1000) std::abort();
}

void callback(const DrawTimeFacts& facts) noexcept
{
    const Facts record{
        uint8_t(facts.in_visual),
        facts.view ? uint8_t(*facts.view) : uint8_t{255},
        facts.domain ? uint8_t(*facts.domain) : uint8_t{255},
        uint8_t(facts.capture != nullptr),
        uint8_t(facts.kind), uint8_t(facts.classification.verdict), uint8_t(facts.classification.type)
    };
    sink(&record);
}

bool install(Sink emit) noexcept
{
    sink = emit;
    auto* base = reinterpret_cast<std::byte*>(GetModuleHandleW(nullptr));
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    const auto* pe = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    rsf_anomaly_hook_site redirect = native_geometry_hook_sites(standard_build)[geometry_site_flush];
    redirect.rva = static_cast<uint32_t>(reinterpret_cast<std::byte*>(&flush_fixture) - base);
    std::memcpy(redirect.expected, reinterpret_cast<const void*>(&flush_fixture), sizeof(redirect.expected));
    return rsf_anomaly_hooks_install(&hooks, base, pe->OptionalHeader.SizeOfImage, &redirect, 1);
}

void observe(bool enabled) noexcept { geometry_set_flush_callback(enabled ? callback : nullptr); }
void flush() noexcept { flush_fixture(); }
bool disable() noexcept { return rsf_anomaly_hooks_disable(&hooks); }
void flush_scoped()
{
    const DrawContextScope context(temporal::ViewId::Scope, temporal::ProjectionDomain::World);
    flush_fixture();
    const DrawContextScope hud(DrawContextScope::InheritView{}, temporal::ProjectionDomain::FirstPerson);
    IdentityRegistry registry;
    const DrawScope scope(registry, std::nullopt, DrawKind::HardwareSkinned, nullptr, nullptr,
        {DrawPass::Main, VisualType::SkeletonST, DrawVerdict::UnidentifiedDynamic});
    flush_fixture();
}

struct Fixture {
    bool (*install)(Sink) noexcept;
    void (*observe)(bool) noexcept;
    void (*flush)() noexcept;
    bool (*flush_scoped)() noexcept;
    bool (*disable)() noexcept;
};
constexpr Fixture fixture{
    install, observe, flush,
    []() noexcept { try { flush_scoped(); return true; } catch (...) { return false; } },
    disable,
};
extern "C" int rsf_anomaly_motion_hooks_fixture(const Fixture*) noexcept;
}

int main()
{
    const auto status = rsf_anomaly_motion_hooks_fixture(&fixture);
    observe(false);
    return status;
}
