/* SPDX-License-Identifier: MIT */
/* AC7 render scopes queue commands into their native list. This C11 launcher keeps that list,
   the native command execution guard and the D3D11 draw calls in one translation unit; Rust
   owns the phases and verdicts. A structured-exception filter on the MSVC C++ exception code
   0xE06D7363 stops what the former catch-all stopped; other faults propagate. */
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#define COBJMACROS
#include <windows.h>
#include <rescaleframe/d3d11_present_bridge.h>
#include "presentation/d3d11_marker_bridge.h"
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_scene.h>
#include "presentation/d3d11_scope_fixture.h"
#include "presentation/present_fixture_loops.h"
#include <d3d11.h>
#include <dxgi1_6.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>

#include <rescaleframe/foreign_guard.h>

#if !defined(_MSC_VER)
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif

_Static_assert(sizeof(rsf_ac7_loop) == 104 && offsetof(rsf_ac7_loop, read_cpu) == 96, "AC7 loop layout");
_Static_assert(sizeof(rsf_ac7_fixture_calls) == 24 && offsetof(rsf_ac7_fixture_calls, run_guarded) == 16,
    "AC7 fixture calls layout");
_Static_assert(sizeof(rsf_ac7_fixture_commands) == 112 && offsetof(rsf_ac7_fixture_commands, execute) == 104,
    "AC7 fixture commands layout");

typedef struct NativeCommand NativeCommand;
struct NativeCommand {
    NativeCommand* next;
    void (*execute)(void*, NativeCommand*);
};
typedef struct NativeList {
    NativeCommand* root;
    NativeCommand** tail;
    bool executing;
    unsigned char padding[3];
    uint32_t count, uid;
} NativeList;
_Static_assert(sizeof(NativeCommand) == 16 && sizeof(NativeList) == 32 &&
    offsetof(NativeList, count) == 0x14 && offsetof(NativeList, uid) == 0x18, "Native command list layout");

static void native_list_reset(NativeList* list)
{
    list->root = NULL;
    list->tail = &list->root;
    list->executing = false;
    list->count = 0;
    list->uid = 1;
}

extern void rsf_d3d11_fixture_run(NativeList*, void (*)(NativeList*, NativeCommand*));

static bool queued_passes_ok = true;

static void run_command(NativeList* owner, NativeCommand* command)
{
    RSF_FOREIGN_TRY {
        command->execute(owner, command);
    } RSF_FOREIGN_CATCH { queued_passes_ok = false; }
}

static void run(void* commands)
{
    rsf_d3d11_fixture_run((NativeList*)commands, run_command);
}

/* Optional: hold one frame's scene submission open, to exercise the render hitch sampler. */
static DWORD submission_stall_ms = 0;
static uint32_t expected_presentation_path = RSF_NATIVE_SCENE_PRESENT_NONE;
static ID3D11DeviceContext* final_context = NULL;

static void queued_pass(void* user, void* chain, const rsf_game_render_pass* pass, uint32_t begin)
{
    (void)user;
    (void)chain;
    queued_passes_ok = queued_passes_ok &&
        rsf_d3d11_fixture_queued_pass(final_context, expected_presentation_path, pass, begin) != 0;
}

typedef struct PresentCommand {
    NativeCommand base;
    IDXGISwapChain* chain;
    bool simulation_pending;
    HRESULT result;
} PresentCommand;

static void present_execute(void* owner, NativeCommand* command)
{
    PresentCommand* present = (PresentCommand*)command;
    uint64_t source = 0;
    (void)owner;
    if (!rsf_d3d11_fixture_window_source(present->chain, expected_presentation_path, &source)) return;
    present->result = IDXGISwapChain_Present(present->chain, 0, 0);
    /* RenderSubmitEnd precedes PresentStart, so PresentEnd retires the token unless the
       game thread's SimulationEnd is still outstanding. */
    queued_passes_ok = queued_passes_ok &&
        rsf_d3d11_fixture_token_matches(source, (uint32_t)present->simulation_pending) != 0;
}

typedef struct EvaluateCommand {
    NativeCommand base;
    ID3D11DeviceContext* context;
    const rsf_game_render_pass* source;
    const rsf_dlss_frame* frame;
    rsf_dlss_result result;
} EvaluateCommand;

static void evaluate_execute(void* owner, NativeCommand* command)
{
    EvaluateCommand* evaluate = (EvaluateCommand*)command;
    (void)owner;
    if (submission_stall_ms) { Sleep(submission_stall_ms); submission_stall_ms = 0; }
    rsf_native_fg_scene(evaluate->source);
    evaluate->result = rsf_native_fg_evaluate(evaluate->context, evaluate->frame);
    rsf_native_fg_scene(NULL);
}

typedef struct SimulationEndCommand {
    NativeCommand base;
    uint64_t source;
} SimulationEndCommand;

static void simulation_end_execute(void* owner, NativeCommand* command)
{
    (void)owner;
    rsf_d3d11_fixture_cpu_event(((SimulationEndCommand*)command)->source, RSF_GAME_CPU_SIMULATION_END);
}

typedef struct FrameCommands {
    rsf_ac7_render_scopes* scopes;
    ID3D11Device* device;
    ID3D11DeviceContext* context;
    IDXGISwapChain* chain;
    ID3D11Texture2D* hudless;
    ID3D11RenderTargetView* hudless_target;
    HWND window;
    rsf_dlss_frame* frame;
    NativeList commands;
    rsf_ac7_render_ticket* frame_ticket;
} FrameCommands;

static const rsf_d3d11_scope_fixture_calls scope_calls = {rsf_ac7_render_scope_open, rsf_ac7_render_scope_close};

static int begin_commands(void* user, const rsf_game_render_pass* source, uint32_t direct)
{
    FrameCommands* frame = (FrameCommands*)user;
    RSF_FOREIGN_TRY {
        native_list_reset(&frame->commands);
        frame->frame_ticket = NULL;
        if (!rsf_d3d11_fixture_scope_begin(&scope_calls, frame->scopes, &frame->commands, source,
            &frame->frame_ticket, direct, &expected_presentation_path)) return 0;
        run(&frame->commands);
        return 1;
    } RSF_FOREIGN_CATCH { return 0; }
}

typedef struct DrawBuffers {
    FrameCommands* frame;
    ID3D11Texture2D* backbuffer;
    ID3D11RenderTargetView* target;
} DrawBuffers;

static void draw_release(DrawBuffers* draw)
{
    if (draw->target) { ID3D11RenderTargetView_Release(draw->target); draw->target = NULL; }
    if (draw->backbuffer) { ID3D11Texture2D_Release(draw->backbuffer); draw->backbuffer = NULL; }
}

static int draw_buffers(void* user, void** backbuffer)
{
    DrawBuffers* draw = (DrawBuffers*)user;
    const FrameCommands* frame = draw->frame;
    RSF_FOREIGN_TRY {
        static const float color[] = {0.25f, 0.2f, 0.6f, 1};
        if (FAILED(IDXGISwapChain_GetBuffer(frame->chain, 0, &IID_ID3D11Texture2D, (void**)&draw->backbuffer)) ||
            FAILED(ID3D11Device_CreateRenderTargetView(frame->device, (ID3D11Resource*)draw->backbuffer, NULL,
                &draw->target))) return 0;
        ID3D11DeviceContext_ClearRenderTargetView(frame->context, draw->target, color);
        ID3D11DeviceContext_ClearRenderTargetView(frame->context, frame->hudless_target, color);
        *backbuffer = draw->backbuffer;
        return 1;
    } RSF_FOREIGN_CATCH { return 0; }
}

static int draw_present(void* user, const rsf_game_render_pass* source, uint32_t direct, uint32_t late_simulation,
    int32_t* evaluated, int32_t* presented)
{
    FrameCommands* frame = (FrameCommands*)user;
    RSF_FOREIGN_TRY {
        EvaluateCommand evaluate = {{NULL, evaluate_execute}, frame->context, source, frame->frame,
            RSF_DLSS_ERROR_NOT_READY};
        PresentCommand present = {{NULL, present_execute}, frame->chain, late_simulation != 0, E_FAIL};
        SimulationEndCommand simulation_end = {{NULL, simulation_end_execute}, source->source_frame_id};
        DrawBuffers buffers = {frame, NULL, NULL};
        const rsf_ac7_fixture_commands packet = {
            &scope_calls, frame->scopes, frame->frame_ticket, &frame->commands,
            frame->chain, frame->hudless, frame->window, frame->frame,
            &evaluate, &present, &simulation_end, &buffers, draw_buffers, run,
        };
        const int recorded = rsf_d3d11_fixture_record(&packet, source, direct, late_simulation);
        draw_release(&buffers);
        if (!recorded) return -1;
        *evaluated = evaluate.result;
        *presented = present.result;
        return (int)queued_passes_ok;
    } RSF_FOREIGN_CATCH { return -1; }
}

static int finish_commands(void* user)
{
    rsf_ac7_render_scopes* scopes = ((FrameCommands*)user)->scopes;
    rsf_ac7_render_scopes_quiesce(scopes);
    return rsf_ac7_render_scopes_destroy(scopes);
}

static void marker_dispatch_target(void* user, const rsf_game_cpu_event* fact)
{
    (void)user;
    rsf_native_fg_cpu(fact);
}

static int marker_dispatch(const rsf_game_cpu_event* event)
{
    return rsf_native_cpu_dispatch(event, marker_dispatch_target, NULL);
}

static const rsf_d3d11_marker_fixture_calls marker_calls = {
    rsf_ac7_fixture_marker_configure,
    rsf_ac7_fixture_marker_stamp,
    rsf_ac7_fixture_marker_frequency,
    marker_dispatch,
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

static LONG WINAPI fixture_exception_filter(EXCEPTION_POINTERS* exception)
{
    void* frames[24] = {0};
    USHORT count, i;
    fprintf(stderr, "fixture exception 0x%08lx at %p\n", exception->ExceptionRecord->ExceptionCode,
        exception->ExceptionRecord->ExceptionAddress);
    count = CaptureStackBackTrace(0, 24, frames, NULL);
    for (i = 0; i < count; ++i) {
        MEMORY_BASIC_INFORMATION region = {0};
        char module[MAX_PATH] = {0};
        VirtualQuery(frames[i], &region, sizeof(region));
        GetModuleFileNameA((HMODULE)region.AllocationBase, module, MAX_PATH);
        fprintf(stderr, "  %s + 0x%llx\n", module,
            (unsigned long long)((uintptr_t)frames[i] - (uintptr_t)region.AllocationBase));
    }
    return EXCEPTION_EXECUTE_HANDLER;
}

void rsf_ac7_fixture_install_exception_filter(void)
{
    SetUnhandledExceptionFilter(fixture_exception_filter);
}

int rsf_ac7_fixture_run_guarded(rsf_ac7_loop* loop, void* device, void* context,
    void* hudless, void* hudless_target, rsf_dlss_frame* frame, uint32_t stall_ms)
{
    rsf_ac7_render_scopes* scopes = NULL;
    FrameCommands commands;
    final_context = (ID3D11DeviceContext*)context;
    submission_stall_ms = stall_ms;
    if (!rsf_ac7_render_scopes_create_passes(42, 8, NULL, queued_pass, NULL, &scopes)) return 1;
    memset(&commands, 0, sizeof(commands));
    commands.scopes = scopes;
    commands.device = (ID3D11Device*)device;
    commands.context = final_context;
    commands.chain = (IDXGISwapChain*)loop->chain;
    commands.hudless = (ID3D11Texture2D*)hudless;
    commands.hudless_target = (ID3D11RenderTargetView*)hudless_target;
    commands.window = (HWND)loop->window;
    commands.frame = frame;
    native_list_reset(&commands.commands);
    loop->user = &commands;
    loop->begin = begin_commands;
    loop->draw_present = draw_present;
    loop->finish = finish_commands;
    loop->status = rsf_native_fg_status_get;
    loop->read_cpu = rsf_native_cpu_read;
    return rsf_d3d11_fixture_run_loop(loop);
}

int main(int argc, char** argv)
{
    const rsf_ac7_fixture_calls calls = {
        rsf_ac7_fixture_install_exception_filter, &marker_calls, rsf_ac7_fixture_run_guarded,
    };
    setvbuf(stdout, NULL, _IONBF, 0);
    return rsf_d3d11_fixture_main(argc, argv, &calls);
}
