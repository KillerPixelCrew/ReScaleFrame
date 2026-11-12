// SPDX-License-Identifier: GPL-3.0-only
#include "native_hooks.h"
#include <windows.h>
#include <array>
#include <cstdio>
#include <cstring>

namespace {
using Function = int(*)(int);
Function original{};
__declspec(noinline) int fixture(int value) { return value + 7; }
__declspec(noinline) int replacement(int value) { return original(value) + 3; }
}
int main() {
    using namespace rsf::anomaly;
    auto base = reinterpret_cast<std::byte*>(GetModuleHandleW(nullptr));
    auto dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    auto pe = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    auto address = reinterpret_cast<std::byte*>(reinterpret_cast<void*>(&fixture));
    HookSite site{};
    site.name = "test executable fixture";
    site.rva = static_cast<uint32_t>(address - base);
    std::memcpy(site.expected.data(), address, site.expected.size());
    site.replacement = reinterpret_cast<void*>(&replacement);
    site.original = reinterpret_cast<void**>(&original);
    HookSet hooks;
    auto call = reinterpret_cast<Function>(address);
    const auto bytes = site.expected;
    site.expected[0] ^= std::byte{1};
    if (hooks.install(base, pe->OptionalHeader.SizeOfImage, {&site, 1}) || call(2) != 9 ||
        std::memcmp(address, bytes.data(), bytes.size())) {
        std::fprintf(stderr, "Mismatched site must refuse without changing instructions\n"); return 1;
    }
    site.expected = bytes;
    std::array<HookSite, 2> batch{site, site};
    batch[1].rva = pe->OptionalHeader.SizeOfImage;
    if (hooks.install(base, pe->OptionalHeader.SizeOfImage, batch) || call(2) != 9 ||
        std::memcmp(address, bytes.data(), bytes.size())) {
        std::fprintf(stderr, "All sites must preflight before the first detour\n"); return 1;
    }
    if (!hooks.install(base, pe->OptionalHeader.SizeOfImage, {&site, 1}) ||
        call(2) != 12 || !original || original(2) != 9) {
        std::fprintf(stderr, "Verified detour and original trampoline must execute\n"); return 1;
    }
    if (!hooks.disable() || call(2) != 9 || std::memcmp(address, bytes.data(), bytes.size())) {
        std::fprintf(stderr, "Disable must restore original instructions\n"); return 1;
    }
    if (!hooks.enable() || call(2) != 12 || !hooks.disable() || call(2) != 9) return 1;
    std::puts("Anomaly guarded native hooks: PASS (actual x64 detour/trampoline/restoration)");
    return 0;
}
