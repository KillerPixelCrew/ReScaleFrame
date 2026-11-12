// SPDX-License-Identifier: GPL-3.0-only
// SR Stage 1 hook contracts without the game:
//  * the shared CRender::rmNear slot: the geometry producer's own detour (redirected onto a fixture function) calls the
//    registered SR callback after the original, and only while one is registered;
//  * sr::install refuses and installs nothing when any Stage 1 guard differs (function prologues, the 17-byte
//    phase_blur guard, the five real call sites), and when the guarded bytes are not inside a real PE image;
//  * Stage 2 also requires the SSAO set_viewport code that encodes its static viewport and guard addresses;
//  * frame_end/report/quiesce are inert while nothing is installed;
//  * SRStage/SRScale parsing (Stage 2 keys, clamping, defaults).
#include "geometry_profile.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include "sr_profile.h"
#include "sr_stage.h"

#include <windows.h>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace {
using namespace rsf::anomaly;
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
    for (const auto& site : sr::sr_function_sites) std::memcpy(image.data() + site.rva, site.expected.data(), 16);
    for (const auto& guard : sr::sr_unique_guards) put_hex(image.data() + guard.rva, guard.expected_hex);
    for (const auto& site : sr::sr_call_sites) std::memcpy(image.data() + site.rva, site.expected.data(), 16);
    put_hex(image.data() + sr::ssao_set_viewport_rva, sr::ssao_set_viewport_code);
}

void rm_near_slot() {
    const auto sites = geometry::native_geometry_hook_sites();
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
    std::vector<std::byte> image(native_image_size);
    CHECK(sr::install(image.data(), native_image_size) && !sr::active());
    sr::StageOptions options{};
    options.stage = 1;
    sr::configure(options, &collect, nullptr);
    CHECK(sr::options().selected());
    // Empty image: the first prologue guard fails.
    CHECK(!sr::install(image.data(), native_image_size) && !sr::active());
    CHECK(logged("guard mismatch at CRenderTarget::phase_scene_prepare"));
    CHECK(!sr::install(image.data(), native_image_size - 0x1000) && logged("image size differs"));
    // Every guard present except one byte past the 16-byte phase_blur prologue (its unique 17-byte guard).
    fill_guards(image);
    image[sr::sr_function_sites[sr::site_phase_blur].rva + 16] ^= std::byte{0xff};
    lines.clear();
    CHECK(!sr::install(image.data(), native_image_size) && logged("guard mismatch at CRenderTarget::phase_blur"));
    // A call site no longer a real call rel32 (an inlined or relinked phase_combine).
    fill_guards(image);
    image[sr::sr_call_sites[3].rva] = std::byte{0x90};
    lines.clear();
    CHECK(!sr::install(image.data(), native_image_size) && logged("guard mismatch at phase_combine -> phase_sunshafts"));
    // All guards match, but the bytes are a heap buffer, not the mapped executable: HookSet refuses before any patch.
    fill_guards(image);
    lines.clear();
    CHECK(!sr::install(image.data(), native_image_size) && logged("guarded detour installation failed") && !sr::active());
    for (const auto& site : sr::sr_function_sites)
        CHECK(std::memcmp(image.data() + site.rva, site.expected.data(), 16) == 0);   // nothing was written
    // Stage 2: the SSAO helper's code is checked first, then every Stage 1 guard.
    options.stage = 2;
    sr::configure(options, &collect, nullptr);
    fill_guards(image);
    image[sr::ssao_set_viewport_rva + 0x42] ^= std::byte{0x01};   // the viewport displacement
    lines.clear();
    CHECK(!sr::install(image.data(), native_image_size) && logged("guard mismatch at set_viewport") && !sr::active());
    fill_guards(image);
    image[sr::sr_function_sites[sr::site_phase_combine].rva] ^= std::byte{0xff};
    lines.clear();
    CHECK(!sr::install(image.data(), native_image_size) && logged("guard mismatch at CRenderTarget::phase_combine"));
    fill_guards(image);
    lines.clear();
    CHECK(!sr::install(image.data(), native_image_size) && logged("guarded detour installation failed") && !sr::active());
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
        {L"0.25", 0.5f, false}, {L"2", 1.0f, false}, {L"0", 0.67f, true}, {L"-1", 0.67f, true},
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
    for (const auto* stage : {L"3", L"-1", L"x"}) {
        WritePrivateProfileStringW(L"Anomaly", L"SRStage", stage, path);   // only Stages 1 and 2 exist
        CHECK(!sr::read_options(path).selected());
    }
    DeleteFileW(path);
}
}

int main() {
    rm_near_slot();
    install_refusals();
    options_from_ini();
    std::printf("%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
