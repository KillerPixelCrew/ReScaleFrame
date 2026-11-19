// SPDX-License-Identifier: MIT
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_scene.h>
#include <rescaleframe/native_window.h>

struct PassCallbacks {
    decltype(&rsf_native_fg_frame) frame;
    decltype(&rsf_native_fg_submission) submission;
    decltype(&rsf_native_scene_pass) scene;
    decltype(&rsf_native_fg_final) final_scene;
    decltype(&rsf_native_scene_texture_binding) texture_binding;
    decltype(&rsf_native_fg_window_end) window_end;
    decltype(&rsf_native_window_scope) window_scope;
};

extern "C" int rsf_core_fg_dispatch_pass(void* context, const rsf_game_render_pass*, uint32_t begin,
    const PassCallbacks*);
extern "C" void rsf_core_fg_capture_scoped(void* context, const rsf_game_render_pass*, const rsf_dlss_frame*,
    decltype(&rsf_native_fg_scene_swap), decltype(&rsf_native_fg_scene), decltype(&rsf_native_fg_capture));

namespace {
void cpu_sink(void*, const rsf_game_cpu_event* event) noexcept { rsf_native_fg_cpu(event); }
}

extern "C" int rsf_native_fg_dispatch_cpu(const rsf_game_cpu_event* event)
{
    return rsf_native_cpu_dispatch(event, cpu_sink, nullptr);
}

extern "C" int rsf_native_fg_dispatch_pass(void* context, const rsf_game_render_pass* pass, uint32_t begin)
{
    static constexpr PassCallbacks callbacks{
        rsf_native_fg_frame, rsf_native_fg_submission, rsf_native_scene_pass, rsf_native_fg_final,
        rsf_native_scene_texture_binding, rsf_native_fg_window_end, rsf_native_window_scope,
    };
    return rsf_core_fg_dispatch_pass(context, pass, begin, &callbacks);
}

extern "C" void rsf_native_fg_capture_scoped(void* context, const rsf_game_render_pass* identity,
    const rsf_dlss_frame* frame)
{
    rsf_core_fg_capture_scoped(context, identity, frame, rsf_native_fg_scene_swap,
        rsf_native_fg_scene, rsf_native_fg_capture);
}
