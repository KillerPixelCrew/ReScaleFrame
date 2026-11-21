// SPDX-License-Identifier: MIT
#include "native_hooks.h"
#include "build_profile.h"
#include <windows.h>
#include <array>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <thread>

#define CHECK(condition) do { if (!(condition)) { \
    std::fprintf(stderr, "HookSet contract failed at line %d: %s\n", __LINE__, #condition); return false; \
} } while (false)

namespace {
using namespace rsf::anomaly;
using Function = int(*)(int);
void* original{};
std::atomic<bool> hold_detour{};
std::atomic<bool> entered{};
std::atomic<bool> resume{};

__declspec(noinline) int fixture(int value) { return value + 7; }
__declspec(noinline) int replacement(int value) {
    if (hold_detour.load()) {
        entered.store(true);
        entered.notify_one();
        resume.wait(false);
    }
    return reinterpret_cast<Function>(original)(value) + 3;
}
__declspec(noinline) int untouched(int value) { return value * 5 + 11; }

struct Image {
    std::byte* base;
    Image(uint32_t size, DWORD protection)
        : base(static_cast<std::byte*>(VirtualAlloc(nullptr, size, MEM_RESERVE | MEM_COMMIT, protection))) {}
    ~Image() { if (base) VirtualFree(base, 0, MEM_RELEASE); }
    IMAGE_DOS_HEADER* write_headers(uint32_t size) const {
        auto* dos = reinterpret_cast<IMAGE_DOS_HEADER*>(base);
        dos->e_magic = IMAGE_DOS_SIGNATURE;
        dos->e_lfanew = 64;
        auto* pe = reinterpret_cast<IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
        pe->Signature = IMAGE_NT_SIGNATURE;
        pe->FileHeader.Machine = IMAGE_FILE_MACHINE_AMD64;
        pe->OptionalHeader.Magic = IMAGE_NT_OPTIONAL_HDR64_MAGIC;
        pe->OptionalHeader.SizeOfImage = size;
        return dos;
    }
};

HookSite fixture_site(const std::byte* base, Function function, void** trampoline) {
    const auto* address = reinterpret_cast<const std::byte*>(reinterpret_cast<void*>(function));
    HookSite site{"test executable fixture", static_cast<uint32_t>(address - base), {},
        reinterpret_cast<void*>(&replacement), trampoline};
    std::memcpy(site.expected.data(), address, site.expected.size());
    return site;
}

bool hook_contract() {
    auto base = reinterpret_cast<std::byte*>(GetModuleHandleW(nullptr));
    auto dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    auto pe = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    const auto image_size = pe->OptionalHeader.SizeOfImage;
    auto site = fixture_site(base, fixture, &original);
    auto address = base + site.rva;
    HookSet hooks;
    auto call = reinterpret_cast<Function>(address);
    const auto bytes = site.expected;
    site.expected.back() ^= std::byte{1};
    CHECK(!hooks.install(base, image_size, {&site, 1}) && !hooks.created() && !original);
    CHECK(call(2) == 9 && !std::memcmp(address, bytes.data(), bytes.size()));
    site.expected = bytes;
    std::array<HookSite, 2> batch{site, site};
    CHECK(!hooks.install(base, image_size, batch));
    batch[1].rva = image_size;
    CHECK(!hooks.install(base, image_size, batch));
    CHECK(call(2) == 9 && !std::memcmp(address, bytes.data(), bytes.size()));
    CHECK(hooks.install(base, image_size, {&site, 1}) && hooks.created());
    CHECK(original && call(2) == 12 && reinterpret_cast<Function>(original)(2) == 9);
    const auto trampoline = original;

    // Hold a detour before its original call while another thread restores the entry.
    hold_detour.store(true);
    int late_result{};
    std::thread in_flight([&] { late_result = call(2); });
    entered.wait(false);
    const bool restored = hooks.disable();
    resume.store(true);
    resume.notify_one();
    in_flight.join();
    hold_detour.store(false);
    CHECK(restored && late_result == 12 && original == trampoline);
    CHECK(call(2) == 9 && !std::memcmp(address, bytes.data(), bytes.size()));

    // The second target's bytes are restored, but it is already registered in MinHook.
    // Its refusal must remove only the first, unqueued registration in the new batch.
    void* fresh{};
    void* conflict{};
    batch[0] = fixture_site(base, untouched, &fresh);
    batch[1] = fixture_site(base, fixture, &conflict);
    HookSet other;
    CHECK(!other.install(base, image_size, batch) && !other.created() && !fresh && !conflict);
    CHECK(original == trampoline && !std::memcmp(base + batch[0].rva, batch[0].expected.data(), bytes.size()));
    const ResolvedHookSite resolved{base + batch[0].rva, batch[0].replacement, &fresh};
    CHECK(other.install_resolved({&resolved, 1}) && fresh);
    CHECK(other.disable() && reinterpret_cast<Function>(fresh)(2) == 21);
    CHECK(hooks.enable() && original == trampoline && call(2) == 12);
    CHECK(hooks.disable() && call(2) == 9);
    return true;
}

bool mapping_contract() {
    SYSTEM_INFO system{};
    GetSystemInfo(&system);
    const auto page = system.dwPageSize;
    const auto size = page * 3;
    const Image image{size, PAGE_READWRITE};
    auto* base = image.base;
    CHECK(base);
    auto* dos = image.write_headers(size);

    void* mapped_original{};
    HookSite site{"mapped code fixture", page + 64, {}, reinterpret_cast<void*>(&replacement), &mapped_original};
    site.expected.fill(std::byte{0x90});
    // Move ECX into EAX, add seven and return. Both code paths return nine for input two.
    const unsigned char code[]{0x8b, 0xc1, 0x83, 0xc0, 0x07, 0xc3};
    std::memcpy(site.expected.data(), code, sizeof(code));
    std::memcpy(base + site.rva, site.expected.data(), site.expected.size());
    const auto split_rva = 2 * page - 8;
    std::memcpy(base + split_rva, site.expected.data(), site.expected.size());
    CHECK(FlushInstructionCache(GetCurrentProcess(), base, size));
    HookSet hooks;
    const auto install = [&] { return hooks.install(base, size, {&site, 1}); };
    const auto protect = [&](uint32_t offset, DWORD mode) {
        DWORD saved{};
        return VirtualProtect(base + offset, page, mode, &saved) != FALSE;
    };
    CHECK(!install()); // Code is readable but non-executable.
    CHECK(protect(page, PAGE_EXECUTE) && !install());
    CHECK(protect(page, PAGE_EXECUTE_READ | PAGE_GUARD) && !install());
    MEMORY_BASIC_INFORMATION info{};
    CHECK(VirtualQuery(base + page, &info, sizeof(info)) && (info.Protect & PAGE_GUARD));
    CHECK(protect(page, PAGE_NOACCESS) && !install());
    CHECK(protect(page, PAGE_EXECUTE_READ));
    dos->e_lfanew = static_cast<LONG>(page - 1);
    CHECK(!install()); // The full NT header must fit a single readable region.
    dos->e_lfanew = 64;
    CHECK(protect(2 * page, PAGE_EXECUTE_READWRITE));
    const auto valid_rva = site.rva;
    site.rva = split_rva;
    CHECK(!install()); // The byte read would cross a VirtualQuery region boundary.
    CHECK(VirtualFree(base + 2 * page, page, MEM_DECOMMIT));
    site.rva = 2 * page;
    CHECK(!install());
    site.rva = valid_rva;
    CHECK(!hooks.install(base + 1, size, {&site, 1})); // The DOS read belongs to a different allocation base.
    CHECK(!hooks.created() && !mapped_original);

    CHECK(protect(0, PAGE_READONLY) && install()); // Readable DOS/NT headers and an RX target.
    CHECK(mapped_original && hooks.disable() && reinterpret_cast<Function>(mapped_original)(2) == 9);
    CHECK(!std::memcmp(base + site.rva, site.expected.data(), site.expected.size()));
    return true;
}

bool profile_contract(const BuildProfile& profile) {
    // Only the copied prologues are mapped. MinHook relocates them, but neither
    // the profile targets nor their original trampolines are called.
    const auto size = profile.image_size();
    const Image image{size, PAGE_EXECUTE_READWRITE};
    CHECK(image.base);
    image.write_headers(size);
    constexpr std::array sr_detours{sr::site_phase_scene_prepare, sr::site_phase_combine,
        sr::site_phase_sunshafts, sr::site_phase_blur, sr::site_sun_cascades};
    constexpr auto count = native_site_count + motion::motion_site_count + sr_detours.size();
    std::array<void*, count> originals{};
    std::array<HookSite, count> sites{};
    size_t index = 0;
    const auto add = [&](const NativeSite& site) {
        std::memcpy(image.base + site.rva, site.expected.data(), site.expected.size());
        sites[index] = {site.name, site.rva, site.expected, reinterpret_cast<void*>(&replacement), &originals[index]};
        ++index;
    };
    for (const auto& site : profile.native->native_sites) add(site);
    for (const auto& site : profile.motion->motion_sites) add(site);
    for (const auto sr_site : sr_detours) add(profile.sr->sr_function_sites[sr_site]);
    CHECK(FlushInstructionCache(GetCurrentProcess(), image.base, size));
    HookSet hooks;
    for (auto& site : sites) {
        site.expected.back() ^= std::byte{1};
        CHECK(!hooks.install(image.base, size, sites) && !hooks.created());
        site.expected.back() ^= std::byte{1};
    }
    for (const auto original_address : originals) CHECK(!original_address);
    CHECK(hooks.install(image.base, size, sites));
    for (size_t i = 0; i < sites.size(); ++i) {
        const auto& site = sites[i];
        CHECK(originals[i] && std::memcmp(image.base + site.rva, site.expected.data(), site.expected.size()));
    }
    CHECK(hooks.disable());
    for (const auto& site : sites)
        CHECK(!std::memcmp(image.base + site.rva, site.expected.data(), site.expected.size()));
    return true;
}
}

int main() {
    if (!hook_contract() || !mapping_contract()) return 1;
    for (const auto& profile : build_profiles) {
        std::printf("Anomaly HookSet profile: %s\n", profile.label);
        if (!profile_contract(profile)) return 1;
    }
    std::puts("Anomaly guarded native hooks: PASS (mapped guards, real detours, rollback and in-flight trampoline)");
    return 0;
}
