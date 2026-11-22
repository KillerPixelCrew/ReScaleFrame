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

void callback(const rsf_anomaly_geometry_draw_facts* facts) noexcept
{
    const Facts record{
        uint8_t(facts->in_visual), facts->view, facts->domain, uint8_t(facts->capture != nullptr),
        facts->kind, facts->verdict, facts->type
    };
    sink(&record);
}

bool install(Sink emit) noexcept
{
    sink = emit;
    auto* base = reinterpret_cast<std::byte*>(GetModuleHandleW(nullptr));
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    const auto* pe = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    rsf_anomaly_hook_site redirect = rsf_anomaly_geometry_hook_sites(0)[geometry_site_flush];
    redirect.rva = static_cast<uint32_t>(reinterpret_cast<std::byte*>(&flush_fixture) - base);
    std::memcpy(redirect.expected, reinterpret_cast<const void*>(&flush_fixture), sizeof(redirect.expected));
    return rsf_anomaly_hooks_install(&hooks, base, pe->OptionalHeader.SizeOfImage, &redirect, 1);
}

void observe(bool enabled) noexcept { rsf_anomaly_geometry_set_flush(enabled ? callback : nullptr); }
void flush() noexcept { flush_fixture(); }
bool disable() noexcept { return rsf_anomaly_hooks_disable(&hooks); }
struct ContextScope {
    rsf_anomaly_geometry_context node{};
    ContextScope(uint8_t view, uint8_t domain, bool inherit) { rsf_anomaly_geometry_context_enter(&node, view, domain, inherit); }
    ~ContextScope() { rsf_anomaly_geometry_context_leave(&node); }
};
struct DrawScope {
    rsf_anomaly_geometry_registry* registry{rsf_anomaly_geometry_registry_create()};
    rsf_anomaly_geometry_visual node{};
    DrawScope()
    {
        node.registry = registry;
        node.kind = RSF_ANOMALY_GEOMETRY_HARDWARE_SKINNED;
        node.pass = RSF_ANOMALY_PASS_MAIN;
        node.type = RSF_ANOMALY_TYPE_SKELETON_ST;
        node.verdict = RSF_ANOMALY_DRAW_UNIDENTIFIED_DYNAMIC;
        rsf_anomaly_geometry_visual_enter(&node);
    }
    ~DrawScope()
    {
        rsf_anomaly_geometry_visual_leave(&node);
        rsf_anomaly_geometry_registry_destroy(registry);
    }
};
void flush_scoped()
{
    const ContextScope context(RSF_ANOMALY_GEOMETRY_SCOPE, RSF_ANOMALY_GEOMETRY_WORLD, false);
    flush_fixture();
    const ContextScope hud(RSF_ANOMALY_GEOMETRY_UNSET, RSF_ANOMALY_GEOMETRY_FIRST_PERSON, true);
    const DrawScope scope;
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
