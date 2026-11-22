// SPDX-License-Identifier: MIT
// SR Stage 1 hook contracts without the game:
//  * the shared CRender::rmNear slot: the geometry producer's own detour (redirected onto a fixture function) calls the
//    registered SR callback after the original, and only while one is registered;
//  * sr::install refuses and installs nothing when any Stage 1 guard differs (function prologues, the 17-byte
//    phase_blur guard, the five real call sites), and when the guarded bytes are not inside a real PE image;
//  * Stage 2 also requires the SSAO set_viewport code that encodes its static viewport and guard addresses;
//  * Stage 3 also requires the set_P and SetMipLODBias guards and, at all six first-person sites, `call set_P` followed
//    by an rmNear call (slot 55 or direct);
//  * frame_end/report/quiesce are inert while nothing is installed;
//  * SRStage/SRScale/SRJitter/SRMipBias parsing (Stage 2 and 3 keys, clamping, defaults).
#include "anomaly_build_select.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include "sr_stage.h"

#include <windows.h>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace {
using namespace rsf::anomaly;
// The build profile under test (argument "standard" or "avx"); every site and address below comes from its tables.
const BuildProfile* B{};
int failures = 0;
#define CHECK(condition) do { if (!(condition)) { std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #condition); ++failures; } } while (0)

int near_calls = 0, callback_calls = 0;
void* callback_self{};
__declspec(noinline) void __fastcall near_fixture(void* self) { ++near_calls; if (!self) std::abort(); }
void callback(void* self) noexcept { ++callback_calls; callback_self = self; }

std::vector<std::string> lines;
void collect(void*, const char* line) { lines.emplace_back(line); }
bool logged(const char* text) {
    for (const auto& line : lines) if (line.find(text) != std::string::npos) return true;
    return false;
}

// A temporary INI: the stage is configured only from [Anomaly] keys, as the cold entry does.
struct TempIni {
    wchar_t path[MAX_PATH]{};
    TempIni() {
        wchar_t directory[MAX_PATH]{};
        GetTempPathW(MAX_PATH, directory);
        GetTempFileNameW(directory, L"rsf", 0, path);
    }
    ~TempIni() { DeleteFileW(path); }
    TempIni(const TempIni&) = delete;
    TempIni& operator=(const TempIni&) = delete;
    void set(const wchar_t* key, const wchar_t* value) const { WritePrivateProfileStringW(L"Anomaly", key, value, path); }
};

void put_hex(std::byte* at, const char* hex) {
    const auto digit = [](char c) { return c <= '9' ? c - '0' : c - 'a' + 10; };
    for (std::size_t i = 0; hex[2 * i]; ++i) at[i] = std::byte(digit(hex[2 * i]) * 16 + digit(hex[2 * i + 1]));
}
void fill_guards(std::vector<std::byte>& image) {
    for (const auto& site : B->sr->sr_function_sites) std::memcpy(image.data() + site.rva, site.expected.data(), 16);
    for (const auto& guard : B->sr->sr_unique_guards) put_hex(image.data() + guard.rva, guard.expected_hex);
    for (const auto& site : B->sr->sr_call_sites) std::memcpy(image.data() + site.rva, site.expected.data(), 16);
    for (const auto& site : B->sr->reconstruction_guards) put_hex(image.data() + site.rva, site.expected_hex);
    put_hex(image.data() + B->sr->ssao_set_viewport_rva, B->sr->ssao_set_viewport_code);
}

void rm_near_slot() {
    const auto sites = geometry::native_geometry_hook_sites(*B);
    auto* base = reinterpret_cast<std::byte*>(GetModuleHandleW(nullptr));
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    const auto* pe = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    HookSite redirect = sites[geometry::geometry_site_rm_near];
    redirect.rva = static_cast<uint32_t>(reinterpret_cast<std::byte*>(&near_fixture) - base);
    std::memcpy(redirect.expected.data(), reinterpret_cast<const void*>(&near_fixture), 16);
    HookSet hooks;
    CHECK(hooks.install(base, pe->OptionalHeader.SizeOfImage, std::span<const HookSite>(&redirect, 1)));
    int self{};
    near_fixture(&self);   // detoured: the geometry rm_near_hook runs the original, then the slot
    CHECK(near_calls == 1 && callback_calls == 0);
    geometry::geometry_set_rm_near_callback(&callback);
    near_fixture(&self);
    CHECK(near_calls == 2 && callback_calls == 1 && callback_self == &self);
    geometry::geometry_set_rm_near_callback(nullptr);
    near_fixture(&self);
    CHECK(near_calls == 3 && callback_calls == 1);
    CHECK(hooks.disable());
    geometry::geometry_set_rm_near_callback(&callback);
    near_fixture(&self);   // the detour is off: nothing but the original runs
    CHECK(near_calls == 4 && callback_calls == 1);
    geometry::geometry_set_rm_near_callback(nullptr);
}

void install_refusals() {
    // Not selected: nothing is kept, nothing to install, nothing refused.
    const TempIni ini;
    ini.set(L"SRStage", L"0");
    CHECK(!sr::configure(ini.path, true, &collect, nullptr));
    std::vector<std::byte> image(B->image_size());
    CHECK(sr::install(image.data(), B->image_size(), *B) && !sr::active());
    ini.set(L"SRStage", L"1");
    CHECK(sr::configure(ini.path, true, &collect, nullptr));
    // Empty image: the first prologue guard fails.
    CHECK(!sr::install(image.data(), B->image_size(), *B) && !sr::active());
    CHECK(logged("guard mismatch at CRenderTarget::phase_scene_prepare"));
    CHECK(!sr::install(image.data(), B->image_size() - 0x1000, *B) && logged("image size differs"));
    // Every guard present except one byte past the 16-byte phase_blur prologue (its unique 17-byte guard).
    fill_guards(image);
    image[B->sr->sr_function_sites[sr::site_phase_blur].rva + 16] ^= std::byte{0xff};
    lines.clear();
    CHECK(!sr::install(image.data(), B->image_size(), *B) && logged("guard mismatch at CRenderTarget::phase_blur"));
    // A call site no longer a real call rel32 (an inlined or relinked phase_combine).
    fill_guards(image);
    image[B->sr->sr_call_sites[3].rva] = std::byte{0x90};
    lines.clear();
    CHECK(!sr::install(image.data(), B->image_size(), *B) && logged("guard mismatch at phase_combine -> phase_sunshafts"));
    // The modern sun wrapper is guarded before any patch, including the distinguishing bytes beyond expected16.
    for (const auto offset : {0u, 29u}) {
        fill_guards(image);
        image[B->sr->sr_function_sites[sr::site_sun_cascades].rva + offset] ^= std::byte{0xff};
        lines.clear();
        CHECK(!sr::install(image.data(), B->image_size(), *B) && logged("guard mismatch at CRender::render_sun_cascades"));
    }
    fill_guards(image);
    image[B->sr->sr_call_sites[5].rva + 1] ^= std::byte{0x01};
    lines.clear();
    CHECK(!sr::install(image.data(), B->image_size(), *B) && logged("guard mismatch at CRender::Render -> render_sun_cascades"));
    // Matching bytes after SizeOfImage cannot prove an in-image call site.
    auto outside = *B;
    auto outside_sr = *B->sr;
    outside.sr = &outside_sr;
    auto& outside_call = outside_sr.sr_call_sites[0];
    outside_call.rva = B->image_size();
    image.resize(B->image_size() + outside_call.expected.size());
    fill_guards(image);
    std::memcpy(image.data() + outside_call.rva, outside_call.expected.data(), outside_call.expected.size());
    lines.clear();
    CHECK(!sr::install(image.data(), B->image_size(), outside) && logged("guard mismatch at") && logged(outside_call.name));
    image.resize(B->image_size());
    // All guards match, but the bytes are a heap buffer, not the mapped executable: HookSet refuses before any patch.
    fill_guards(image);
    lines.clear();
    CHECK(!sr::install(image.data(), B->image_size(), *B) && logged("guarded detour installation failed") && !sr::active());
    for (const auto& site : B->sr->sr_function_sites)
        CHECK(std::memcmp(image.data() + site.rva, site.expected.data(), 16) == 0);   // nothing was written
    // Stage 2: the SSAO helper's code is checked first, then every Stage 1 guard.
    ini.set(L"SRStage", L"2");
    CHECK(sr::configure(ini.path, true, &collect, nullptr));
    fill_guards(image);
    image[B->sr->ssao_set_viewport_rva + std::strlen(B->sr->ssao_set_viewport_code) / 2 - 4] ^= std::byte{0x01};   // the viewport displacement
    lines.clear();
    CHECK(!sr::install(image.data(), B->image_size(), *B) && logged("guard mismatch at set_viewport") && !sr::active());
    fill_guards(image);
    image[B->sr->sr_function_sites[sr::site_phase_combine].rva] ^= std::byte{0xff};
    lines.clear();
    CHECK(!sr::install(image.data(), B->image_size(), *B) && logged("guard mismatch at CRenderTarget::phase_combine"));
    fill_guards(image);
    lines.clear();
    CHECK(!sr::install(image.data(), B->image_size(), *B) && logged("guarded detour installation failed") && !sr::active());
    // Inert while not installed.
    lines.clear();
    sr::frame_end();
    sr::report(&collect, nullptr);
    CHECK(lines.empty() && sr::quiesce());
}

void stage3_install_refusals() {
    const TempIni ini;
    ini.set(L"SRStage", L"3");
    CHECK(sr::configure(ini.path, true, &collect, nullptr));
    std::vector<std::byte> image(B->image_size());
    const auto refused_at = [&](const char* text) {
        lines.clear();
        const bool ok = !sr::install(image.data(), B->image_size(), *B) && !sr::active() && logged(text);
        if (!ok) for (const auto& line : lines) std::printf("  log: %s\n", line.c_str());
        return ok;
    };
    // set_P is called from the adapter: its 28-byte unique guard must match.
    fill_guards(image);
    image[B->sr->sr_function_sites[sr::site_set_p].rva + 20] ^= std::byte{0x01};
    CHECK(refused_at("guard mismatch at R_xforms::set_P"));
    fill_guards(image);
    image[B->sr->sr_function_sites[sr::site_set_mip_lod_bias].rva + 12] ^= std::byte{0x01};   // the m_uiMipLODBias load
    CHECK(refused_at("guard mismatch at dx10SamplerStateCache::SetMipLODBias"));
    // Each first-person site must still upload with `call set_P` and then call rmNear.
    const auto put_call = [&](uint32_t at, uint32_t target) {
        image[at] = std::byte{0xe8};
        const int32_t displacement = static_cast<int32_t>(static_cast<int64_t>(target) - (static_cast<int64_t>(at) + 5));
        std::memcpy(image.data() + at + 1, &displacement, sizeof(displacement));
    };
    const auto fill_sites = [&] {
        fill_guards(image);
        for (const auto& site : B->sr->sr_first_person_sites) {
            put_call(site.set_p_call, B->sr->sr_function_sites[sr::site_set_p].rva);
            // CParticleEffect::Render and accum_point call rmNear directly (sr_function_sites 13 and 14); the others use slot 55.
            if (site.function_rva == B->sr->sr_function_sites[13].rva || site.function_rva == B->sr->sr_function_sites[14].rva)
                put_call(site.rm_near_call, B->sr->sr_function_sites[sr::site_rm_near].rva);
            else put_hex(image.data() + site.rm_near_call, "ff90b8010000");
        }
    };
    fill_sites();
    CHECK(refused_at("guarded detour installation failed"));   // every Stage 3 guard matched; the heap buffer is refused later
    fill_sites();
    put_call(B->sr->sr_first_person_sites[2].set_p_call, B->sr->sr_function_sites[sr::site_set_p].rva + 0x10);   // calls elsewhere
    CHECK(refused_at("guard mismatch at r_dsgraph_render_sorted"));
    fill_sites();
    image[B->sr->sr_first_person_sites[0].rm_near_call + 2] = std::byte{0xc0};   // call [rax+0x1c0]: slot 56, rmFar
    CHECK(refused_at("guard mismatch at r_dsgraph_render_hud "));
    fill_sites();
    put_call(B->sr->sr_first_person_sites[5].rm_near_call, B->sr->sr_function_sites[sr::site_rm_far].rva);   // direct call to rmFar
    CHECK(refused_at("guard mismatch at CRenderTarget::accum_point"));
    // Every layout-dependent reconstruction body must match before temporal hooks can run.
    for (const auto& site : B->sr->reconstruction_guards) {
        fill_sites();
        image[site.rva + std::strlen(site.expected_hex) / 2 - 1] ^= std::byte{0x01};
        CHECK(refused_at(site.name));
    }
    // Inert while not installed: no jitter is published.
    const auto uv = sr::current_jitter_uv();
    CHECK(uv.current_u == 0 && uv.current_v == 0 && uv.previous_u == 0 && uv.previous_v == 0 && !sr::current_jitter().applied);
}
}

int main(int argc, char** argv) {
    B = &testing::selected_build(argc, argv);
    testing::announce(*B);
    rm_near_slot();
    install_refusals();
    stage3_install_refusals();
    std::printf("%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
