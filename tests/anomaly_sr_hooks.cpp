// SPDX-License-Identifier: GPL-3.0-only
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
#include "adapter_options.h"

#include <windows.h>
#include <cstdio>
#include <cstring>
#include <filesystem>
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

void put_hex(std::byte* at, const char* hex) {
    const auto digit = [](char c) { return c <= '9' ? c - '0' : c - 'a' + 10; };
    for (std::size_t i = 0; hex[2 * i]; ++i) at[i] = std::byte(digit(hex[2 * i]) * 16 + digit(hex[2 * i + 1]));
}
void fill_guards(std::vector<std::byte>& image) {
    for (const auto& site : B->sr->sr_function_sites) std::memcpy(image.data() + site.rva, site.expected.data(), 16);
    for (const auto& guard : B->sr->sr_unique_guards) put_hex(image.data() + guard.rva, guard.expected_hex);
    for (const auto& site : B->sr->sr_call_sites) std::memcpy(image.data() + site.rva, site.expected.data(), 16);
    for (const auto& site : B->sr->sun_depth_guards) put_hex(image.data() + site.rva, site.expected_hex);
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
    // Not selected: nothing to install, nothing refused.
    sr::configure({}, &collect, nullptr);
    std::vector<std::byte> image(B->image_size());
    CHECK(sr::install(image.data(), B->image_size(), *B) && !sr::active());
    sr::StageOptions options{};
    options.stage = 1;
    sr::configure(options, &collect, nullptr);
    CHECK(sr::options().selected());
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
    // All guards match, but the bytes are a heap buffer, not the mapped executable: HookSet refuses before any patch.
    fill_guards(image);
    lines.clear();
    CHECK(!sr::install(image.data(), B->image_size(), *B) && logged("guarded detour installation failed") && !sr::active());
    for (const auto& site : B->sr->sr_function_sites)
        CHECK(std::memcmp(image.data() + site.rva, site.expected.data(), 16) == 0);   // nothing was written
    // Stage 2: the SSAO helper's code is checked first, then every Stage 1 guard.
    options.stage = 2;
    sr::configure(options, &collect, nullptr);
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
    sr::configure({}, nullptr, nullptr);
}

void options_from_ini() {
    wchar_t directory[MAX_PATH]{}, path[MAX_PATH]{};
    GetTempPathW(MAX_PATH, directory);
    GetTempFileNameW(directory, L"rsf", 0, path);
    CHECK(!sr::read_options(path).selected());
    WritePrivateProfileStringW(L"Anomaly", L"SRStage", L"1", path);
    WritePrivateProfileStringW(L"Anomaly", L"SRStageCapture", L"1", path);
    WritePrivateProfileStringW(L"Anomaly", L"SRStageCaptureCount", L"99", path);
    WritePrivateProfileStringW(L"Anomaly", L"SRStageCaptureInterval", L"0", path);
    auto options = sr::read_options(path);
    CHECK(options.stage == 1 && options.capture && options.capture_start == 600 && options.capture_count == 16 &&
        options.capture_interval == 1);
    CHECK(options.scale == 0.67f && options.scale_defaulted && options.ini == path);   // SRScale missing: the default
    WritePrivateProfileStringW(L"Anomaly", L"SRStageCapture", L"0", path);
    WritePrivateProfileStringW(L"Anomaly", L"SRStage", L"2", path);
    options = sr::read_options(path);
    CHECK(options.selected() && options.stage == 2 && options.scale == 0.67f && options.scale_defaulted);
    const struct { const wchar_t* text; float scale; bool defaulted; } scales[] = {
        {L"0.5", 0.5f, false}, {L"0.77", 0.77f, false}, {L"1.0", 1.0f, false}, {L" 0.6 ", 0.6f, false},
        {L"0.25", 0.33f, false}, {L"2", 1.0f, false}, {L"0", 0.67f, true}, {L"-1", 0.67f, true},
        {L"abc", 0.67f, true}, {L"0.7x", 0.67f, true}, {L"nan", 0.67f, true}};
    for (const auto& item : scales) {
        WritePrivateProfileStringW(L"Anomaly", L"SRScale", item.text, path);
        bool defaulted = false;
        const float scale = sr::read_scale(path, &defaulted);
        if (scale != item.scale || defaulted != item.defaulted) std::printf("SRScale=%ls -> %f (defaulted %d)\n", item.text, scale, defaulted);
        CHECK(scale == item.scale && defaulted == item.defaulted);
        CHECK(sr::read_options(path).scale == item.scale);
    }
    CHECK(sr::read_scale(nullptr) == 0.67f);
    for (const auto* stage : {L"6", L"-1", L"x"}) {
        WritePrivateProfileStringW(L"Anomaly", L"SRStage", stage, path);   // only Stages 1 to 5 exist
        CHECK(!sr::read_options(path).selected());
    }
    // Stages 4 and 5: SR inputs and temporal SR keys, the vendor layout beside the INI, the shared frontend SR keys.
    WritePrivateProfileStringW(L"Anomaly", L"SRStage", L"5", path);
    options = sr::read_options(path);
    CHECK(options.stage == 5 && options.world_window() && options.temporal() && options.sr_inputs() && options.selected());
    CHECK(!options.exposure_auto && options.first_person_zero && options.cut.distance == 4.0f && options.cut.degrees == 45.0f &&
        !options.require_signature && options.capture_count == 16);
    const auto beside = (std::filesystem::path(path).parent_path() / L"ReScaleFrame").wstring();
    CHECK(options.streamline_dir == beside + L"\\streamline" && options.fidelityfx_dir == beside + L"\\fidelityfx" &&
        options.xess_dir == beside + L"\\xess");
    CHECK(options.settings.enabled && options.settings.backend == sr::backend_dlss && options.settings.quality == 1 && options.settings.preset == 0);
    WritePrivateProfileStringW(L"Anomaly", L"SRExposure", L"auto", path);
    WritePrivateProfileStringW(L"Anomaly", L"SRFirstPersonZero", L"0", path);
    WritePrivateProfileStringW(L"Anomaly", L"SRCutDistance", L"0.001", path);
    WritePrivateProfileStringW(L"Anomaly", L"SRCutAngle", L"x", path);
    WritePrivateProfileStringW(L"Anomaly", L"SRStreamlineDir", L"C:\\sl", path);
    WritePrivateProfileStringW(L"Anomaly", L"SR", L"0", path);
    WritePrivateProfileStringW(L"Anomaly", L"SRBackend", L"5", path);
    WritePrivateProfileStringW(L"Anomaly", L"SRQuality", L"5", path);    // past ultra performance: not offered, kept at Quality
    WritePrivateProfileStringW(L"Anomaly", L"DLSSPreset", L"6", path);
    options = sr::read_options(path);
    CHECK(options.exposure_auto && !options.first_person_zero && options.cut.distance == 0.1f && options.cut.degrees == 45.0f &&
        options.streamline_dir == L"C:\\sl" && options.xess_dir == beside + L"\\xess");
    CHECK(!options.settings.enabled && options.settings.backend == sr::backend_xess && options.settings.quality == 1 &&
        options.settings.preset == 6);
    WritePrivateProfileStringW(L"Anomaly", L"SRBackend", L"2", path);    // FSR2: not offered by this adapter
    CHECK(sr::read_options(path).settings.backend == sr::backend_dlss);
    WritePrivateProfileStringW(L"Anomaly", L"SRStage", L"4", path);
    options = sr::read_options(path);
    CHECK(options.stage == 4 && options.sr_inputs() && options.temporal() && options.world_window());
    WritePrivateProfileStringW(L"Anomaly", L"SRStage", L"3", path);
    CHECK(!sr::read_options(path).sr_inputs());
    // Stage 3: SRJitter (default 1), SRMipBias (offset, default 0, clamped [-2, 1], "off"), consecutive capture count.
    WritePrivateProfileStringW(L"Anomaly", L"SRScale", L"0.67", path);
    WritePrivateProfileStringW(L"Anomaly", L"SRStage", L"3", path);
    WritePrivateProfileStringW(L"Anomaly", L"SRStageCaptureCount", nullptr, path);
    options = sr::read_options(path);
    CHECK(options.stage == 3 && options.world_window() && options.selected() && options.jitter && options.mip_bias &&
        options.mip_bias_offset == 0.0f && options.capture_count == 8);
    WritePrivateProfileStringW(L"Anomaly", L"SRStageCaptureCount", L"99", path);
    WritePrivateProfileStringW(L"Anomaly", L"SRJitter", L"0", path);
    options = sr::read_options(path);
    CHECK(options.capture_count == 32 && !options.jitter);
    const struct { const wchar_t* text; float offset; bool enabled; } biases[] = {
        {L"-0.3", -0.3f, true}, {L"0", 0.0f, true}, {L" -1 ", -1.0f, true}, {L"-5", -2.0f, true}, {L"3", 1.0f, true},
        {L"off", 0.0f, false}, {L" OFF ", 0.0f, false}, {L"offset", 0.0f, true}, {L"abc", 0.0f, true}, {L"-0.5x", 0.0f, true},
        {L"nan", 0.0f, true}, {L"", 0.0f, true}};
    for (const auto& item : biases) {
        WritePrivateProfileStringW(L"Anomaly", L"SRMipBias", item.text, path);
        bool enabled = !item.enabled;
        const float offset = sr::read_mip_bias(path, &enabled);
        if (offset != item.offset || enabled != item.enabled) std::printf("SRMipBias=%ls -> %f (enabled %d)\n", item.text, offset, enabled);
        CHECK(offset == item.offset && enabled == item.enabled);
        options = sr::read_options(path);
        CHECK(options.mip_bias_offset == item.offset && options.mip_bias == item.enabled);
    }
    CHECK(sr::read_mip_bias(nullptr) == 0.0f);
    WritePrivateProfileStringW(L"Anomaly", L"SRStage", L"2", path);   // Stage 2 keeps its single-frame capture limit
    CHECK(sr::read_options(path).capture_count == 16);
    DeleteFileW(path);
}

void stage3_install_refusals() {
    sr::StageOptions options{};
    options.stage = 3;
    options.sun_depth_margin = 0;   // guards still authorize a later live enable
    sr::configure(options, &collect, nullptr);
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
    // These guards are required even if the live margin starts at zero, so enabling it later remains safe.
    for (const auto& site : B->sr->sun_depth_guards) {
        fill_sites();
        image[site.rva + std::strlen(site.expected_hex) / 2 - 1] ^= std::byte{0x01};
        CHECK(refused_at(site.name));
    }
    // Inert while not installed: no jitter is published.
    const auto uv = sr::current_jitter_uv();
    CHECK(uv.current_u == 0 && uv.current_v == 0 && uv.previous_u == 0 && uv.previous_v == 0 && !sr::current_jitter().applied);
    sr::configure({}, nullptr, nullptr);
}
void sun_margin_option() {
    wchar_t directory[MAX_PATH]{}, path[MAX_PATH]{};
    GetTempPathW(MAX_PATH, directory);
    GetTempFileNameW(directory, L"rsf", 0, path);
    CHECK(sr::read_options(path).sun_depth_margin == 5 && sr::read_options(nullptr).sun_depth_margin == 5);
    options::registry().clear();
    options::ini_path() = path;
    sr::configure({}, nullptr, nullptr);
    const options::Option* control{};
    for (const auto& option : options::registry())
        if (std::string_view(option.label) == "Sun shadow margin with jitter") control = &option;
    CHECK(control && !control->toggle && !control->restart && control->minimum == 0 && control->maximum == 20 &&
        control->step == 1 && control->divisor == 10000 && control->get() == 5);
    if (control) {
        for (const int32_t value : {0, 5, 20, 7, -1, 21}) {
            control->set(value);
            const auto expected = sr::clamp_sun_depth_margin(value);
            CHECK(control->get() == expected && sr::options().sun_depth_margin == static_cast<uint32_t>(expected));
            CHECK(sr::read_options(path).sun_depth_margin == static_cast<uint32_t>(expected));
            CHECK(GetPrivateProfileIntW(L"Anomaly", L"SRSunDepthMargin", 99, path) == static_cast<uint32_t>(expected));
        }
    }
    for (const auto* text : {L"x", L"nan", L"5x", L""}) {
        WritePrivateProfileStringW(L"Anomaly", L"SRSunDepthMargin", text, path);
        CHECK(sr::read_options(path).sun_depth_margin == 5);
    }
    options::ini_path().clear();
    DeleteFileW(path);
}

}

int main(int argc, char** argv) {
    B = &testing::selected_build(argc, argv);
    testing::announce(*B);
    rm_near_slot();
    install_refusals();
    stage3_install_refusals();
    options_from_ini();
    sun_margin_option();
    std::printf("%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
