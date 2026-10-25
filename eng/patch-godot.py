"""Apply the bounded engine bridge to the pinned, ignored Godot source checkout."""
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
ENGINE = ROOT / "references/godot/engine"
REVISION = "ed1daf0bf001b61586d9930840f2f1394092c079"
if subprocess.check_output(["git", "-C", str(ENGINE), "rev-parse", "HEAD"], text=True).strip() != REVISION:
    raise SystemExit("Godot revision differs from the bridge baseline")

INCLUDE = '#include "modules/modules_enabled.gen.h"\n#ifdef MODULE_RESCALEFRAME_ENABLED\n#include "rsf_godot_engine.h"\n#endif\n'
FILES = (
    "core/input/input.cpp",
    "main/main.cpp",
    "platform/windows/os_windows.cpp",
    "platform/windows/display_server_windows.cpp",
    "drivers/d3d12/rendering_device_driver_d3d12.cpp",
    "drivers/d3d12/rendering_device_driver_d3d12.h",
    "servers/rendering/renderer_rd/forward_clustered/render_forward_clustered.cpp",
    "servers/rendering/renderer_viewport.cpp",
    "servers/rendering/rendering_device_graph.cpp",
)
original = {
    path: subprocess.check_output(["git", "-C", str(ENGINE), "show", f"HEAD:{path}"], text=True, encoding="utf-8")
    for path in FILES
}
planned = original.copy()
validated = {}


def replace(path, old, new):
    content = planned[path]
    if content.count(old) != 1:
        raise SystemExit(f"Unexpected Godot source at {path}: {old!r}")
    planned[path] = content.replace(old, new)


for path in (
    "core/input/input.cpp",
    "drivers/d3d12/rendering_device_driver_d3d12.cpp",
    "servers/rendering/renderer_rd/forward_clustered/render_forward_clustered.cpp",
    "servers/rendering/renderer_viewport.cpp",
):
    content = planned[path]
    at = content.index("#include ")
    planned[path] = content[:at] + INCLUDE + "\n" + content[at:]

replace("main/main.cpp", '#include "modules/modules_enabled.gen.h" // For mono.', INCLUDE.rstrip())
replace("servers/rendering/rendering_device_graph.cpp", '\tcommand->type = RecordedCommand::TYPE_DRIVER_CALLBACK;',
        '\tcommand->type = RecordedCommand::TYPE_DRIVER_CALLBACK;\n'
        '\t// Native callbacks can issue compute, graphics and copy commands.\n'
        '\t// An empty stage mask cannot make their declared resource accesses visible.\n'
        '\tcommand->self_stages = RDD::PIPELINE_STAGE_ALL_COMMANDS_BIT;')
replace("main/main.cpp", '\titerating++;', '\titerating++;\n#ifdef MODULE_RESCALEFRAME_ENABLED\n\tRSFGodot::cpu_begin();\n#endif')
replace("main/main.cpp", '\n\t\t\t\tRenderingServer::get_singleton()->draw(wants_present, scaled_step);',
        '\n#ifdef MODULE_RESCALEFRAME_ENABLED\n\t\t\t\tRSFGodot::queue_frame();\n#endif\n\t\t\t\tRenderingServer::get_singleton()->draw(wants_present, scaled_step);')
replace("main/main.cpp", '\n\t\t\tRenderingServer::get_singleton()->draw(wants_present, scaled_step);',
        '\n#ifdef MODULE_RESCALEFRAME_ENABLED\n\t\t\tRSFGodot::queue_frame();\n#endif\n\t\t\tRenderingServer::get_singleton()->draw(wants_present, scaled_step);')
replace("main/main.cpp", '\titerating--;', '#ifdef MODULE_RESCALEFRAME_ENABLED\n\tRSFGodot::cpu_end();\n#endif\n\titerating--;')
replace("core/input/input.cpp", 'void Input::_parse_input_event_impl(const Ref<InputEvent> &p_event, bool p_is_emulated) {',
        'void Input::_parse_input_event_impl(const Ref<InputEvent> &p_event, bool p_is_emulated) {\n#ifdef MODULE_RESCALEFRAME_ENABLED\n'
        '\tif (!p_is_emulated) {\n\t\tuint32_t kind = 0;\n'
        '\t\tif (Object::cast_to<InputEventKey>(p_event.ptr())) kind = 1;\n'
        '\t\telse if (Object::cast_to<InputEventMouse>(p_event.ptr()) || Object::cast_to<InputEventScreenTouch>(p_event.ptr()) || Object::cast_to<InputEventScreenDrag>(p_event.ptr())) kind = 2;\n'
        '\t\telse if (Object::cast_to<InputEventJoypadButton>(p_event.ptr()) || Object::cast_to<InputEventJoypadMotion>(p_event.ptr())) kind = 4;\n'
        '\t\tRSFGodot::input_event(kind);\n\t}\n#endif')
replace("drivers/d3d12/rendering_device_driver_d3d12.h", '\tvirtual void command_buffer_end(CommandBufferID p_cmd_buffer) override final;',
        '\tvoid *rescaleframe_begin(CommandBufferID id);\n\tvoid rescaleframe_end(CommandBufferID id);\n\tvirtual void command_buffer_end(CommandBufferID p_cmd_buffer) override final;')
replace("drivers/d3d12/rendering_device_driver_d3d12.cpp", '\t\tcommand_queue->d3d_queue->ExecuteCommandLists(command_lists.size(), command_lists.ptr());',
        '\t\tcommand_queue->d3d_queue->ExecuteCommandLists(command_lists.size(), command_lists.ptr());\n#ifdef MODULE_RESCALEFRAME_ENABLED\n\t\tRSFGodot::submitted(command_queue->d3d_queue.Get(), reinterpret_cast<void *const *>(command_lists.ptr()), command_lists.size());\n#endif')
replace("drivers/d3d12/rendering_device_driver_d3d12.cpp", '\t\tres = swap_chain->d3d_swap_chain->Present(swap_chain->sync_interval, swap_chain->present_flags);',
        '#ifdef MODULE_RESCALEFRAME_ENABLED\n\t\tRSFGodot::present(swap_chain->d3d_swap_chain.Get());\n#endif\n\t\tres = swap_chain->d3d_swap_chain->Present(swap_chain->sync_interval, swap_chain->present_flags);')
replace("servers/rendering/renderer_rd/forward_clustered/render_forward_clustered.cpp", '\t\tRENDER_TIMESTAMP("Tonemap");',
        '#ifdef MODULE_RESCALEFRAME_ENABLED\n\t\tRSFGodot::upscale(p_render_data, time_step);\n#endif\n\t\tRENDER_TIMESTAMP("Tonemap");')
replace("servers/rendering/renderer_rd/forward_clustered/render_forward_clustered.cpp", '\t\t_render_buffers_post_process_and_tonemap(p_render_data);',
        '\t\t_render_buffers_post_process_and_tonemap(p_render_data);\n#ifdef MODULE_RESCALEFRAME_ENABLED\n\t\tRSFGodot::hudless(p_render_data);\n#endif')
replace("servers/rendering/renderer_viewport.cpp", '\t\t\tp_viewport->internal_size = Size2(render_width, render_height);',
        '\t\t\tp_viewport->internal_size = Size2(render_width, render_height);\n#ifdef MODULE_RESCALEFRAME_ENABLED\n\t\t\tRSFGodot::render_size(p_viewport->render_target, Size2i(target_width, target_height), p_viewport->internal_size);\n#endif')
replace("servers/rendering/renderer_viewport.cpp", '\t\t\trb_config.set_internal_size(Size2i(render_width, render_height));',
        '\t\t\trb_config.set_internal_size(p_viewport->internal_size);')
previous_plan = planned.copy()
replace("drivers/d3d12/rendering_device_driver_d3d12.cpp", '\t\tif (create_for_composition) {\n\t\t\tres = context_driver->dxgi_factory_get()->CreateSwapChainForComposition',
        '#ifdef MODULE_RESCALEFRAME_ENABLED\n'
        '\t\tif (!create_for_composition && RSFGodot::create_swapchain(context_driver->dxgi_factory_get(), command_queue->d3d_queue.Get(), surface->hwnd, &swap_chain_desc, reinterpret_cast<void **>(swap_chain_1.GetAddressOf()))) {\n'
        '\t\t\tres = S_OK;\n\t\t} else\n#endif\n'
        '\t\tif (create_for_composition) {\n\t\t\tres = context_driver->dxgi_factory_get()->CreateSwapChainForComposition')
previous_camera_plan = planned.copy()
replace("servers/rendering/renderer_viewport.cpp", '\tviewport->camera = p_camera;',
        '#ifdef MODULE_RESCALEFRAME_ENABLED\n\tif (viewport->camera != p_camera) RSFGodot::camera_changed(viewport->render_target);\n#endif\n\tviewport->camera = p_camera;')
previous_queue_plan = planned.copy()
for indent in ("\t\t\t", "\t\t\t\t"):
    replace("main/main.cpp", "\n" + indent + "RSFGodot::queue_frame();\n",
            "\n" + indent + "RSFGodot::queue_frame(wants_present);\n")
previous_pacing_plan = planned.copy()
for path in ("platform/windows/os_windows.cpp", "platform/windows/display_server_windows.cpp"):
    at = planned[path].index("#include ")
    planned[path] = planned[path][:at] + INCLUDE + "\n" + planned[path][at:]
replace("platform/windows/os_windows.cpp", '\t\tDisplayServer::get_singleton()->process_events(); // get rid of pending events',
        '#ifdef MODULE_RESCALEFRAME_ENABLED\n\t\tRSFGodot::cpu_begin();\n#endif\n'
        '\t\tDisplayServer::get_singleton()->process_events(); // get rid of pending events')
replace("platform/windows/display_server_windows.cpp", '\t\t\t\t_THREAD_SAFE_UNLOCK_\n\t\t\t\t_process_key_events();',
        '\t\t\t\t_THREAD_SAFE_UNLOCK_\n#ifdef MODULE_RESCALEFRAME_ENABLED\n'
        '\t\t\t\tif (!Main::is_iterating()) RSFGodot::cpu_begin();\n#endif\n\t\t\t\t_process_key_events();')
replace("main/main.cpp", '\tRSFGodot::cpu_begin();', '\tRSFGodot::cpu_begin();\n\tRSFGodot::cpu_simulation_begin();')
changed = set(subprocess.check_output(["git", "-C", str(ENGINE), "diff", "--name-only"], text=True).splitlines())
if changed - set(FILES):
    raise SystemExit("Unexpected reference changes: " + ", ".join(sorted(changed - set(FILES))))
for path in FILES:
    current = (ENGINE / path).read_text(encoding="utf-8")
    validated[path] = current
    if current not in (original[path], previous_plan[path], previous_camera_plan[path], previous_queue_plan[path], previous_pacing_plan[path], planned[path]):
        raise SystemExit("Reference has unrelated changes: " + path)
    if "--check" in sys.argv and current != planned[path]:
        raise SystemExit("Bridge patch is missing: " + path)
if "--check" not in sys.argv:
    for path in FILES:
        target = ENGINE / path
        if target.read_text(encoding="utf-8") != validated[path]:
            raise SystemExit("Reference changed during patch validation: " + path)
        if validated[path] != planned[path]:
            target.write_text(planned[path], encoding="utf-8", newline="\n")
print("Godot bridge patches verified for " + REVISION)
