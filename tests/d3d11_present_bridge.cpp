// SPDX-License-Identifier: MIT
#include <rescaleframe/d3d11_present_bridge.h>
#include "presentation/d3d11_marker_bridge.h"
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_scene.h>
#include <rescaleframe/native_window.h>
#include "presentation/d3d11_scope_fixture.h"
#include "presentation/present_fixture_loops.h"
#include <d3d11.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstring>
#include <cstddef>
using Microsoft::WRL::ComPtr;
static_assert(sizeof(rsf_ac7_loop) == 104 && offsetof(rsf_ac7_loop, read_cpu) == 96);
static_assert(sizeof(rsf_ac7_fixture_calls) == 32 && offsetof(rsf_ac7_fixture_calls, run_guarded) == 24);
struct NativeCommand {
    NativeCommand* next = nullptr;
    void (*execute)(void*, NativeCommand*) = nullptr;
};
struct NativeList {
    NativeCommand* root = nullptr;
    NativeCommand** tail = &root;
    bool executing = false;
    unsigned char padding[3]{};
    uint32_t count = 0, uid = 1;

    void reset() {
        root = nullptr;
        tail = &root;
        executing = false;
        count = 0;
        uid = 1;
    }
};
static_assert(sizeof(NativeCommand) == 16 && sizeof(NativeList) == 32 &&
    offsetof(NativeList, count) == 0x14 && offsetof(NativeList, uid) == 0x18);
extern "C" void rsf_d3d11_fixture_append(NativeList*, NativeCommand*);
extern "C" void rsf_d3d11_fixture_run(NativeList*, void (*)(NativeList*, NativeCommand*));

bool queued_passes_ok = true;
void append(NativeList& list, NativeCommand& command) {
    rsf_d3d11_fixture_append(&list, &command);
}
void run(NativeList& list) {
    rsf_d3d11_fixture_run(&list, [](NativeList* owner, NativeCommand* command) noexcept {
        try { command->execute(owner, command); }
        catch (...) { queued_passes_ok = false; }
    });
}
// Optional: hold one frame's scene submission open, to exercise the render hitch sampler.
DWORD submission_stall_ms = 0;
uint32_t expected_presentation_path = RSF_NATIVE_SCENE_PRESENT_NONE;
ID3D11DeviceContext* final_context = nullptr;
void queued_pass(void*, void*, const rsf_game_render_pass* pass, uint32_t begin) {
    if (pass->role == RSF_GAME_RENDER_SUBMISSION) rsf_native_fg_submission(pass, begin);
    if (pass->role == RSF_GAME_RENDER_FRAME) rsf_native_fg_frame(pass, begin);
    if (pass->role == RSF_GAME_RENDER_FINAL_SCENE) {
        queued_passes_ok &= rsf_native_scene_pass(pass, begin) != 0;
        if (!begin && final_context) rsf_native_fg_final(final_context, pass);
    }
    if (pass->role == RSF_GAME_RENDER_TEXTURE_BINDING && !begin)
        queued_passes_ok &= rsf_native_scene_texture_binding(pass) != 0;
    if (pass->role == RSF_GAME_RENDER_WINDOW) {
        if (begin) {
            const auto expected = expected_presentation_path == RSF_NATIVE_SCENE_PRESENT_DIRECT ?
                RSF_NATIVE_SCENE_PRESENT_DIRECT : RSF_NATIVE_SCENE_PRESENT_NONE;
            queued_passes_ok &= rsf_native_scene_present_path(pass) == expected;
            auto stale = *pass; ++stale.source_frame_id;
            queued_passes_ok &= rsf_native_scene_present_path(&stale) == RSF_NATIVE_SCENE_PRESENT_NONE;
            auto other_viewport = *pass; ++other_viewport.viewport_key;
            queued_passes_ok &= rsf_native_scene_present_path(&other_viewport) == RSF_NATIVE_SCENE_PRESENT_NONE;
        }
        if (!begin) rsf_native_fg_window_end(pass);
        queued_passes_ok &= rsf_native_window_scope(pass, begin) != 0;
    }
}
struct PresentCommand : NativeCommand {
    IDXGISwapChain* chain = nullptr;
    bool simulation_pending = false;
    HRESULT result = E_FAIL;
    PresentCommand() { execute = [](void*, NativeCommand* command) {
        auto& present = *static_cast<PresentCommand*>(command);
        rsf_game_render_pass window{}; window.struct_size = sizeof(window);
        if (!rsf_native_window_match(present.chain, &window) ||
            rsf_native_scene_present_path(&window) != expected_presentation_path) return;
        present.result = present.chain->Present(0, 0);
        // RenderSubmitEnd precedes PresentStart, so PresentEnd retires the token unless the
        // game thread's SimulationEnd is still outstanding.
        queued_passes_ok &= rsf_d3d11_fixture_token_matches(window.source_frame_id, uint32_t(present.simulation_pending)) != 0;
    }; }
};
struct EvaluateCommand : NativeCommand {
    ID3D11DeviceContext* context = nullptr;
    const rsf_game_render_pass* source = nullptr;
    const rsf_dlss_frame* frame = nullptr;
    rsf_dlss_result result = RSF_DLSS_ERROR_NOT_READY;
    EvaluateCommand() { execute = [](void*, NativeCommand* command) {
        auto& evaluate = *static_cast<EvaluateCommand*>(command);
        if (submission_stall_ms) { Sleep(submission_stall_ms); submission_stall_ms = 0; }
        rsf_native_fg_scene(evaluate.source);
        evaluate.result = rsf_native_fg_evaluate(evaluate.context, evaluate.frame);
        rsf_native_fg_scene(nullptr);
    }; }
};
struct SimulationEndCommand : NativeCommand {
    uint64_t source = 0;
    SimulationEndCommand() { execute = [](void*, NativeCommand* command) {
        rsf_d3d11_fixture_cpu_event(static_cast<SimulationEndCommand*>(command)->source, RSF_GAME_CPU_SIMULATION_END);
    }; }
};
uint64_t marker_reentry_source{};
uint32_t marker_reentries{};
void log_message(void*, const char* message) {
    std::printf("SDK: %s\n", message);
    if (marker_reentry_source && std::strstr(message,"actual graphics submit begin")) {
        const auto source = marker_reentry_source; marker_reentry_source = 0; ++marker_reentries;
        rsf_native_latency_render(source,1); // synchronous logger reentry must not open a second interval
    }
}
// AC7 render scopes queue commands into their native list. Keep that list and
// the native command execution guard in the same translation unit.
struct FrameCommands {
    rsf_ac7_render_scopes* scopes;
    ID3D11Device* device;
    ID3D11DeviceContext* context;
    IDXGISwapChain* chain;
    ID3D11Texture2D* hudless;
    ID3D11RenderTargetView* hudless_target;
    HWND window;
    rsf_dlss_frame* frame;
    NativeList commands;
    rsf_ac7_render_ticket* frame_ticket = nullptr;

    FrameCommands(rsf_ac7_render_scopes* scopes, ID3D11Device* device, ID3D11DeviceContext* context,
        IDXGISwapChain* chain, ID3D11Texture2D* hudless, ID3D11RenderTargetView* hudless_target,
        HWND window, rsf_dlss_frame* frame)
        : scopes(scopes), device(device), context(context), chain(chain), hudless(hudless),
          hudless_target(hudless_target), window(window), frame(frame) {}
};

const rsf_d3d11_scope_fixture_calls scope_calls{rsf_ac7_render_scope_open, rsf_ac7_render_scope_close};

int begin_commands(void* user, const rsf_game_render_pass* source, uint32_t direct) noexcept try {
    auto& frame = *static_cast<FrameCommands*>(user);
    expected_presentation_path = direct ? RSF_NATIVE_SCENE_PRESENT_DIRECT : RSF_NATIVE_SCENE_PRESENT_SAMPLED;
    frame.commands.reset();
    frame.frame_ticket = nullptr;
    if (!rsf_d3d11_fixture_scope_begin(&scope_calls, frame.scopes, &frame.commands, source, &frame.frame_ticket)) return 0;
    run(frame.commands);
    return 1;
} catch (...) {
    return 0;
}

int draw_present(void* user, const rsf_game_render_pass* source, uint32_t direct, uint32_t late_simulation,
    int32_t* evaluated, int32_t* presented) noexcept try {
    auto& frame = *static_cast<FrameCommands*>(user);
    auto& commands = frame.commands;
    rsf_ac7_render_ticket *submission_ticket = nullptr, *window_ticket = nullptr;
    if (!rsf_d3d11_fixture_scope_scene(&scope_calls, frame.scopes, &commands, source, &submission_ticket)) return -1;
    frame.frame->reset = source->source_frame_id == 2;
    EvaluateCommand evaluate; evaluate.context = frame.context; evaluate.source = source; evaluate.frame = frame.frame;
    append(commands, evaluate);
    ComPtr<ID3D11Texture2D> backbuffer; ComPtr<ID3D11RenderTargetView> target;
    if (FAILED(frame.chain->GetBuffer(0, IID_PPV_ARGS(&backbuffer))) ||
        FAILED(frame.device->CreateRenderTargetView(backbuffer.Get(), nullptr, &target))) return -1;
    const float color[]{0.25f, 0.2f, 0.6f, 1};
    frame.context->ClearRenderTargetView(target.Get(), color);
    frame.context->ClearRenderTargetView(frame.hudless_target, color);
    if (!rsf_d3d11_fixture_scope_window(&scope_calls, frame.scopes, &commands, source,
        submission_ticket, backbuffer.Get(), frame.hudless, frame.window, frame.chain, direct, &window_ticket)) return -1;
    PresentCommand present; present.chain = frame.chain; present.simulation_pending = late_simulation != 0;
    append(commands, present);
    if (!rsf_ac7_render_scope_close(window_ticket, &commands)) return -1;
    SimulationEndCommand simulation_end; simulation_end.source = source->source_frame_id;
    if (late_simulation) append(commands, simulation_end);
    if (!rsf_ac7_render_scope_close(frame.frame_ticket, &commands)) return -1;
    run(commands);
    *evaluated = evaluate.result;
    *presented = present.result;
    return int(queued_passes_ok);
} catch (...) {
    return -1;
}

int finish_commands(void* user) noexcept {
    auto* scopes = static_cast<FrameCommands*>(user)->scopes;
    rsf_ac7_render_scopes_quiesce(scopes);
    return rsf_ac7_render_scopes_destroy(scopes);
}
namespace {
const rsf_d3d11_marker_fixture_calls marker_calls{
    []() {
        rsf_native_fg_options options{sizeof(options),RSF_FG_OFF,1,RSF_REFLEX_OFF,0,1};
        rsf_native_fg_options_set(&options);
        rsf_native_fg_set_log(log_message, nullptr);
    },
    []() -> uint64_t { LARGE_INTEGER qpc{}; QueryPerformanceCounter(&qpc); return uint64_t(qpc.QuadPart); },
    []() -> uint64_t { LARGE_INTEGER frequency{}; QueryPerformanceFrequency(&frequency); return uint64_t(frequency.QuadPart); },
    [](const rsf_game_cpu_event* event) {
        return rsf_native_cpu_dispatch(event, [](void*, const rsf_game_cpu_event* fact) { rsf_native_fg_cpu(fact); }, nullptr);
    },
    rsf_native_cpu_read,
    rsf_native_cpu_counters_get,
    rsf_native_fg_frame,
    rsf_native_fg_submission,
    rsf_native_fg_reflex_ordering_set,
    rsf_native_latency_render,
    rsf_native_latency_present,
    rsf_d3d11_present_abort,
    rsf_native_latency_frame_get,
    rsf_d3d11_present_generation,
    [](uint64_t frame) { return rsf_streamline_host_token(rsf_d3d11_present_host(), frame); },
    [](uint64_t* count) {
        rsf_streamline_latency_status status{}; status.struct_size = sizeof(status);
        const auto result = rsf_streamline_host_latency_status(rsf_d3d11_present_host(), &status);
        *count = status.sleep_calls;
        return result;
    },
    [](uint64_t frame) { marker_reentry_source = frame; },
    [] { return marker_reentries; },
};
} // namespace

extern "C" void rsf_ac7_fixture_install_exception_filter() {
    SetUnhandledExceptionFilter([](EXCEPTION_POINTERS* exception) -> LONG {
        std::fprintf(stderr, "fixture exception 0x%08lx at %p\n", exception->ExceptionRecord->ExceptionCode,
            exception->ExceptionRecord->ExceptionAddress);
        void* frames[24]{}; const auto count = CaptureStackBackTrace(0, 24, frames, nullptr);
        for (USHORT i = 0; i < count; ++i) {
            MEMORY_BASIC_INFORMATION region{}; VirtualQuery(frames[i], &region, sizeof(region));
            char module[MAX_PATH]{}; GetModuleFileNameA(static_cast<HMODULE>(region.AllocationBase), module, MAX_PATH);
            std::fprintf(stderr, "  %s + 0x%llx\n", module,
                static_cast<unsigned long long>(uintptr_t(frames[i]) - uintptr_t(region.AllocationBase)));
        }
        return EXCEPTION_EXECUTE_HANDLER;
    });
}

extern "C" void rsf_ac7_fixture_log(void* user, const char* message) {
    log_message(user, message);
}

extern "C" int rsf_ac7_fixture_run_markers(void* chain) {
    return rsf_d3d11_marker_fixture(chain, &marker_calls);
}

extern "C" int rsf_ac7_fixture_run_guarded(rsf_ac7_loop* loop, void* device, void* context,
    void* hudless, void* hudless_target, rsf_dlss_frame* frame, uint32_t stall_ms) {
    final_context = static_cast<ID3D11DeviceContext*>(context);
    submission_stall_ms = stall_ms;
    rsf_ac7_render_scopes* scopes = nullptr;
    if (!rsf_ac7_render_scopes_create_passes(42, 8, nullptr, queued_pass, nullptr, &scopes)) return 1;
    FrameCommands commands{scopes, static_cast<ID3D11Device*>(device),
        final_context, static_cast<IDXGISwapChain*>(loop->chain), static_cast<ID3D11Texture2D*>(hudless),
        static_cast<ID3D11RenderTargetView*>(hudless_target), static_cast<HWND>(loop->window), frame};
    loop->user = &commands;
    loop->begin = begin_commands;
    loop->draw_present = draw_present;
    loop->finish = finish_commands;
    loop->status = rsf_native_fg_status_get;
    loop->read_cpu = rsf_native_cpu_read;
    return rsf_d3d11_fixture_run_loop(loop);
}

int main(int argc, char** argv) {
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    const rsf_ac7_fixture_calls calls{
        rsf_ac7_fixture_install_exception_filter, rsf_ac7_fixture_log,
        rsf_ac7_fixture_run_markers, rsf_ac7_fixture_run_guarded,
    };
    return rsf_d3d11_fixture_main(argc, argv, &calls);
}
