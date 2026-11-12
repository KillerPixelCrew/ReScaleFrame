/* SPDX-License-Identifier: GPL-3.0-only */
// rsf_get_d3d11_fg_api through a dllimport of the real Runtime DLL, as an adapter uses it: the
// size/version refusal contract, a complete table, and graphics-free behaviour of the entries
// that need no device (CPU ordering, scene/window identity, source binding, inputs query).
#include <rescaleframe/d3d11_fg_api.h>
#include <rescaleframe/d3d11_present_api.h>
#include <windows.h>
#include <cstdio>
#include <cstring>
#include <cstdint>
#include <initializer_list>

#ifdef RSF_RUNTIME_BUILD
#error This contract must import the real Runtime DLL, not compile as its owner.
#endif

namespace {
int failures = 0;
void check(bool condition, const char* message) {
    if (!condition) { ++failures; std::fprintf(stderr, "FAIL: %s\n", message); }
}
void refuses_without_writes(uint32_t size, uint32_t version, rsf_result expected) {
    rsf_d3d11_fg_api api;
    std::memset(&api, 0xa5, sizeof(api));
    api.struct_size = size; api.abi_version = version;
    unsigned char before[sizeof(api)];
    std::memcpy(before, &api, sizeof(api));
    check(rsf_get_d3d11_fg_api(&api) == expected, "deterministic ABI refusal result");
    check(std::memcmp(before, &api, sizeof(api)) == 0, "refusal preserves every caller byte");
}
uint64_t now() { LARGE_INTEGER value{}; QueryPerformanceCounter(&value); return uint64_t(value.QuadPart); }
uint64_t frequency() { LARGE_INTEGER value{}; QueryPerformanceFrequency(&value); return uint64_t(value.QuadPart); }
}

int main() {
    check(rsf_get_d3d11_fg_api(nullptr) == RSF_ERROR_INVALID_ARGUMENT, "null query is INVALID_ARGUMENT");
    for (const uint32_t size : {0u, 4u, 8u, uint32_t(sizeof(rsf_d3d11_fg_api) - 1)}) {
        refuses_without_writes(size, RSF_D3D11_FG_API_VERSION, RSF_ERROR_INVALID_ARGUMENT);
        refuses_without_writes(size, UINT32_MAX, RSF_ERROR_INVALID_ARGUMENT);   // size wins over version
    }
    for (const uint32_t version : {0u, RSF_D3D11_FG_API_VERSION + 1u, UINT32_MAX})
        refuses_without_writes(sizeof(rsf_d3d11_fg_api), version, RSF_ERROR_ABI_MISMATCH);

    // A larger caller capacity: the known prefix is filled, the tail is untouched.
    struct Larger { rsf_d3d11_fg_api api; unsigned char tail[32]; } larger;
    std::memset(&larger, 0x5a, sizeof(larger));
    larger.api.struct_size = sizeof(larger); larger.api.abi_version = RSF_D3D11_FG_API_VERSION;
    check(rsf_get_d3d11_fg_api(&larger.api) == RSF_OK, "larger capacity accepted");
    check(larger.api.struct_size == sizeof(rsf_d3d11_fg_api), "struct_size reports the runtime table");
    bool tail_intact = true;
    for (const auto byte : larger.tail) tail_intact &= byte == 0x5a;
    check(tail_intact, "bytes beyond the table untouched");

    rsf_d3d11_fg_api api{};
    api.struct_size = sizeof(api); api.abi_version = RSF_D3D11_FG_API_VERSION;
    check(rsf_get_d3d11_fg_api(&api) == RSF_OK, "query OK");
    check(api.set_log && api.options && api.status && api.reflex_ordering && api.cpu && api.pass &&
        api.bind_source && api.capture && api.inputs_ready && api.dump_inputs && api.prepare && api.retire &&
        api.present_event && api.latency_event, "every entry populated");
    // The same owner the present table and AC7 use, not a copy.
    check(api.prepare == &rsf_native_fg_prepare && api.retire == &rsf_native_fg_retire &&
        api.options == &rsf_native_fg_options_set && api.status == &rsf_native_fg_status_get &&
        api.bind_source == &rsf_native_fg_bind_source, "entries are the Runtime's native FG owner");

    // Status without a presentation owner: available 0, effective Off, requested options echoed.
    rsf_native_fg_options options{sizeof(options), RSF_FG_FIXED, 2, RSF_REFLEX_ON, 8333, 0};
    api.options(&options);
    rsf_native_fg_status status{}; status.struct_size = sizeof(status);
    check(api.status(&status) == 1 && !status.available && status.vendor.effective_mode == RSF_FG_OFF &&
        status.requested.generated_frames == 2 && status.requested.frame_limit_us == 8333, "status before a bridge owner");
    rsf_native_fg_status short_status{}; short_status.struct_size = sizeof(short_status) - 1;
    check(api.status(&short_status) == 0, "short status refused");

    // CPU ordering is recorded even with no bridge; out-of-order stages are refused.
    const uint64_t session = 77, f = frequency();
    auto event = [&](uint32_t stage, uint64_t id) {
        rsf_game_cpu_event value{sizeof(value), stage, session, id, now(), f, 1, 0, 0};
        return api.cpu(&value);
    };
    check(event(RSF_GAME_CPU_FRAME_BEGIN, 1) == 1, "frame begin accepted");
    check(event(RSF_GAME_CPU_SIMULATION_END, 1) == 0, "simulation end before input refused");
    event(RSF_GAME_CPU_FRAME_END, 1);   // closes the failed record
    check(event(RSF_GAME_CPU_FRAME_BEGIN, 2) == 1 && event(RSF_GAME_CPU_PACING, 2) == 1 &&
        event(RSF_GAME_CPU_INPUT_SAMPLE, 2) == 1 && event(RSF_GAME_CPU_SIMULATION_BEGIN, 2) == 1 &&
        event(RSF_GAME_CPU_SIMULATION_END, 2) == 1 && event(RSF_GAME_CPU_FRAME_END, 2) == 1, "ordered frame accepted");
    check(event(RSF_GAME_CPU_FRAME_BEGIN, 2) == 0, "a repeated frame id is refused");
    rsf_game_cpu_event short_event{sizeof(short_event) - 1, RSF_GAME_CPU_FRAME_BEGIN, session, 3, now(), f, 1, 0, 0};
    check(api.cpu(&short_event) == 0, "short CPU event refused");

    // Render passes: short structures are refused; WINDOW scope needs its identity fields.
    rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass) - 1; pass.role = RSF_GAME_RENDER_WINDOW;
    check(api.pass(nullptr, &pass, 1) == 0 && api.pass(nullptr, nullptr, 1) == 0, "short or null pass refused");
    pass.struct_size = sizeof(pass); pass.session_id = session; pass.source_frame_id = 2;
    check(api.pass(nullptr, &pass, 1) == 0, "WINDOW without scope/window/viewport/swapchain refused");
    rsf_game_render_pass final_scene{}; final_scene.struct_size = sizeof(final_scene); final_scene.role = RSF_GAME_RENDER_FINAL_SCENE;
    final_scene.session_id = session; final_scene.source_frame_id = 2;
    check(api.pass(nullptr, &final_scene, 1) == 0, "FINAL_SCENE without primary/after-simulation identity refused");
    rsf_game_render_pass frame_pass{}; frame_pass.struct_size = sizeof(frame_pass); frame_pass.role = RSF_GAME_RENDER_FRAME;
    check(api.pass(nullptr, &frame_pass, 1) == 1 && api.pass(nullptr, &frame_pass, 0) == 1, "FRAME without a bridge is a no-op");

    // Source binding and inputs: nothing is held without a device; binding and clearing are safe.
    rsf_game_render_pass identity{}; identity.struct_size = sizeof(identity); identity.source_frame_id = 5;
    api.bind_source(&identity); api.bind_source(nullptr);
    rsf_game_render_pass short_identity = identity; short_identity.struct_size = sizeof(short_identity) - 1;
    api.bind_source(&short_identity); api.bind_source(nullptr);
    check(api.inputs_ready(0) == 0 && api.inputs_ready(5) == 0, "no inputs held without a capture");
    check(api.dump_inputs(nullptr, 5, "x", "y") == 0, "dump without inputs writes nothing");
    api.capture(nullptr, &identity, nullptr);   // refused without a context: no crash, no state

    // The presentation table is a separate, unchanged owner.
    rsf_d3d11_present_api present{}; present.struct_size = sizeof(present); present.abi_version = RSF_D3D11_PRESENT_API_VERSION;
    check(rsf_get_d3d11_present_api(&present) == RSF_OK && !present.has_owner(), "present API intact, no owner");

    if (failures) { std::fprintf(stderr, "%d failure(s)\n", failures); return 1; }
    std::puts("PASS: rsf_get_d3d11_fg_api contract through the real Runtime DLL");
    return 0;
}
