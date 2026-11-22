// SPDX-License-Identifier: MIT
#include <rescaleframe/d3d11_present_bridge.h>
#include "presentation/d3d11_marker_bridge.h"
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_scene.h>
#include "presentation/d3d11_scope_fixture.h"
#include "presentation/present_fixture_loops.h"
#include <d3d11.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstddef>
using Microsoft::WRL::ComPtr;
static_assert(sizeof(rsf_ac7_loop) == 104 && offsetof(rsf_ac7_loop, read_cpu) == 96);
static_assert(sizeof(rsf_ac7_fixture_calls) == 24 && offsetof(rsf_ac7_fixture_calls, run_guarded) == 16);
static_assert(sizeof(rsf_ac7_fixture_commands) == 112 && offsetof(rsf_ac7_fixture_commands, execute) == 104);
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
extern "C" void rsf_d3d11_fixture_run(NativeList*, void (*)(NativeList*, NativeCommand*));

bool queued_passes_ok = true;
void run(void* commands) {
    auto& list = *static_cast<NativeList*>(commands);
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
    queued_passes_ok &= rsf_d3d11_fixture_queued_pass(final_context, expected_presentation_path, pass, begin) != 0;
}
struct PresentCommand : NativeCommand {
    IDXGISwapChain* chain = nullptr;
    bool simulation_pending = false;
    HRESULT result = E_FAIL;
    PresentCommand() { execute = [](void*, NativeCommand* command) {
        auto& present = *static_cast<PresentCommand*>(command);
        uint64_t source = 0;
        if (!rsf_d3d11_fixture_window_source(present.chain, expected_presentation_path, &source)) return;
        present.result = present.chain->Present(0, 0);
        // RenderSubmitEnd precedes PresentStart, so PresentEnd retires the token unless the
        // game thread's SimulationEnd is still outstanding.
        queued_passes_ok &= rsf_d3d11_fixture_token_matches(source, uint32_t(present.simulation_pending)) != 0;
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
    frame.commands.reset();
    frame.frame_ticket = nullptr;
    if (!rsf_d3d11_fixture_scope_begin(&scope_calls, frame.scopes, &frame.commands, source, &frame.frame_ticket,
        direct, &expected_presentation_path)) return 0;
    run(&frame.commands);
    return 1;
} catch (...) {
    return 0;
}

struct DrawBuffers {
    FrameCommands* frame;
    ComPtr<ID3D11Texture2D> backbuffer;
    ComPtr<ID3D11RenderTargetView> target;
};

int draw_buffers(void* user, void** backbuffer) noexcept try {
    auto& draw = *static_cast<DrawBuffers*>(user);
    const auto& frame = *draw.frame;
    if (FAILED(frame.chain->GetBuffer(0, IID_PPV_ARGS(&draw.backbuffer))) ||
        FAILED(frame.device->CreateRenderTargetView(draw.backbuffer.Get(), nullptr, &draw.target))) return 0;
    const float color[]{0.25f, 0.2f, 0.6f, 1};
    frame.context->ClearRenderTargetView(draw.target.Get(), color);
    frame.context->ClearRenderTargetView(frame.hudless_target, color);
    *backbuffer = draw.backbuffer.Get();
    return 1;
} catch (...) {
    return 0;
}

int draw_present(void* user, const rsf_game_render_pass* source, uint32_t direct, uint32_t late_simulation,
    int32_t* evaluated, int32_t* presented) noexcept try {
    auto& frame = *static_cast<FrameCommands*>(user);
    EvaluateCommand evaluate; evaluate.context = frame.context; evaluate.source = source; evaluate.frame = frame.frame;
    PresentCommand present; present.chain = frame.chain; present.simulation_pending = late_simulation != 0;
    SimulationEndCommand simulation_end; simulation_end.source = source->source_frame_id;
    DrawBuffers buffers{&frame, {}, {}};
    const rsf_ac7_fixture_commands packet{
        &scope_calls, frame.scopes, frame.frame_ticket, &frame.commands,
        frame.chain, frame.hudless, frame.window, frame.frame,
        static_cast<NativeCommand*>(&evaluate), static_cast<NativeCommand*>(&present),
        static_cast<NativeCommand*>(&simulation_end), &buffers, draw_buffers, run,
    };
    if (!rsf_d3d11_fixture_record(&packet, source, direct, late_simulation)) return -1;
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
    rsf_ac7_fixture_marker_configure,
    rsf_ac7_fixture_marker_stamp,
    rsf_ac7_fixture_marker_frequency,
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
    rsf_ac7_fixture_marker_token,
    rsf_ac7_fixture_marker_sleep_count,
    rsf_ac7_fixture_marker_reenter,
    rsf_ac7_fixture_marker_reentries,
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
        rsf_ac7_fixture_install_exception_filter, &marker_calls, rsf_ac7_fixture_run_guarded,
    };
    return rsf_d3d11_fixture_main(argc, argv, &calls);
}
