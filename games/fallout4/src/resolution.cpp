// SPDX-License-Identifier: GPL-3.0-only
#include "resolution.h"
#include <windows.h>
#include <atomic>
#include <cmath>
#include <cstring>

namespace rsf::fallout4 {
namespace {
constexpr uintptr_t state_rva = 0x3d871a0;
std::atomic<uint32_t> owners{};
thread_local bool output_owned{};
thread_local float saved_jitter[2]{};
uintptr_t image() { return uintptr_t(GetModuleHandleW(nullptr)); }
bool copy(uintptr_t address, void* destination, size_t size) noexcept {
    SIZE_T copied{};
    return ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<const void*>(address), destination, size, &copied) && copied == size;
}
template<size_t N> bool bytes(uintptr_t rva, const unsigned char (&expected)[N]) noexcept {
    unsigned char actual[N]{};
    return copy(image() + rva, actual, N) && !std::memcmp(actual, expected, N);
}
}
bool verify_resolution() noexcept {
    // Native temporal update reads the AE DWORD TAA fields and writes jitter+4/+8.
    constexpr unsigned char taa[]{0x83,0xb9,0xac,0,0,0,0};
    constexpr unsigned char jitter[]{0xf3,0x0f,0x11,0x7f,0x04,0xf3,0x0f,0x11,0x5f,0x08};
    return bytes(0x182d586, taa) && bytes(0x182d647, jitter);
}
bool native_resolution_inputs(const rsf_game_render_pass& pass) noexcept {
    // Until reduced-raster consumer restoration is verified, never label native
    // inputs as a selected reduced quality. Native AA uses the game's own jitter.
    return pass.camera_valid && pass.render_rect[0] == 0 && pass.render_rect[1] == 0 &&
        pass.render_rect[2] == pass.output_rect[2] && pass.render_rect[3] == pass.output_rect[3] &&
        pass.camera.render_width == pass.camera.output_width && pass.camera.render_height == pass.camera.output_height;
}
bool begin_output_resolution() noexcept {
    if (output_owned) return false;
    const auto address = image() + state_rva + 4;
    if (!copy(address, saved_jitter, sizeof(saved_jitter)) ||
        !std::isfinite(saved_jitter[0]) || !std::isfinite(saved_jitter[1])) return false;
    const float zero[2]{};
    SIZE_T written{};
    if (!WriteProcessMemory(GetCurrentProcess(), reinterpret_cast<void*>(address), zero, sizeof(zero), &written) || written != sizeof(zero)) return false;
    output_owned = true; owners.fetch_add(1, std::memory_order_acq_rel);
    return true;
}
void end_output_resolution() noexcept {
    if (!output_owned) return;
    const auto address = image() + state_rva + 4;
    float current[2]{};
    // Restore only our exact output-scope write; a foreign owner keeps its update.
    if (!copy(address, current, sizeof(current))) return;
    if (current[0] == 0 && current[1] == 0) {
        SIZE_T written{};
        if (!WriteProcessMemory(GetCurrentProcess(), reinterpret_cast<void*>(address), saved_jitter,
            sizeof(saved_jitter), &written) || written != sizeof(saved_jitter)) return;
    }
    output_owned = false; owners.fetch_sub(1, std::memory_order_acq_rel);
}
bool resolution_busy() noexcept { return owners.load(std::memory_order_acquire) != 0; }
}
