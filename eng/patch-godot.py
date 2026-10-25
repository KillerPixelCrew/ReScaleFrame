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
    "drivers/vulkan/rendering_device_driver_vulkan.h",
    "drivers/vulkan/rendering_device_driver_vulkan.cpp",
    "drivers/vulkan/rendering_context_driver_vulkan.cpp",
    "drivers/vulkan/rendering_context_driver_vulkan.h",
    "platform/windows/rendering_context_driver_vulkan_windows.cpp",
    "platform/windows/rendering_context_driver_vulkan_windows.h",
    "servers/rendering/renderer_rd/forward_clustered/render_forward_clustered.cpp",
    "servers/rendering/renderer_rd/storage_rd/render_scene_buffers_rd.cpp",
    "servers/rendering/renderer_viewport.cpp",
    "servers/rendering/rendering_device_graph.cpp",
    "thirdparty/amd-fsr2/shaders/ffx_fsr2_callbacks_glsl.h",
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
previous_vulkan_plan = planned.copy()
replace("drivers/vulkan/rendering_device_driver_vulkan.h", '\tvirtual void command_buffer_end(CommandBufferID p_cmd_buffer) override final;',
        '\tvoid *rescaleframe_begin(CommandBufferID id);\n'
        '\tstatic uint64_t rescaleframe_instance_proc();\n\tstatic uint64_t rescaleframe_device_proc();\n'
        '\tvirtual void command_buffer_end(CommandBufferID p_cmd_buffer) override final;')
previous_copy_usage_plan = planned.copy()
path = "servers/rendering/renderer_rd/storage_rd/render_scene_buffers_rd.cpp"
at = planned[path].index("#include ")
planned[path] = planned[path][:at] + '#include "modules/modules_enabled.gen.h"\n\n' + planned[path][at:]
replace(path, '\t\tusage_bits |= RD::TEXTURE_USAGE_INPUT_ATTACHMENT_BIT;\n\t\tcreate_texture(RB_SCOPE_BUFFERS, RB_TEX_COLOR_UPSCALED',
        '\t\tusage_bits |= RD::TEXTURE_USAGE_INPUT_ATTACHMENT_BIT;\n#ifdef MODULE_RESCALEFRAME_ENABLED\n'
        '\t\t// The native SR callback reinserts its accepted output by GPU copy.\n'
        '\t\tusage_bits |= RD::TEXTURE_USAGE_CAN_COPY_TO_BIT;\n#endif\n'
        '\t\tcreate_texture(RB_SCOPE_BUFFERS, RB_TEX_COLOR_UPSCALED')
previous_validation_plan = planned.copy()
# The Windows command-line entry can leave OS_Windows::hInstance null. Match
# Godot's window-class registration fallback when creating the Vulkan surface.
replace("platform/windows/display_server_windows.cpp", '\t\twpd.vulkan.instance = hInstance;',
        '\t\twpd.vulkan.instance = hInstance ? hInstance : GetModuleHandle(nullptr);')
# Match the formats allocated by Godot's vendored FSR2 resource descriptions.
path = "thirdparty/amd-fsr2/shaders/ffx_fsr2_callbacks_glsl.h"
replace(path, 'FSR2_BIND_UAV_DILATED_DEPTH, r16f)', 'FSR2_BIND_UAV_DILATED_DEPTH, r32f)')
replace(path, 'FSR2_BIND_UAV_PREPARED_INPUT_COLOR, rgba16)', 'FSR2_BIND_UAV_PREPARED_INPUT_COLOR, rgba16f)')
previous_capture_plan = planned.copy()
replace("servers/rendering/renderer_rd/storage_rd/render_scene_buffers_rd.cpp",
        '\tuint32_t usage_bits = RD::TEXTURE_USAGE_SAMPLING_BIT | RD::TEXTURE_USAGE_COLOR_ATTACHMENT_BIT | RD::TEXTURE_USAGE_INPUT_ATTACHMENT_BIT;',
        '\tuint32_t usage_bits = RD::TEXTURE_USAGE_SAMPLING_BIT | RD::TEXTURE_USAGE_COLOR_ATTACHMENT_BIT | RD::TEXTURE_USAGE_INPUT_ATTACHMENT_BIT;\n'
        '#ifdef MODULE_RESCALEFRAME_ENABLED\n\t// Bounded native diagnostics read linear scene colour after its producer.\n'
        '\tusage_bits |= RD::TEXTURE_USAGE_CAN_COPY_FROM_BIT;\n#endif')
previous_wsi_plan = planned.copy()
path = "drivers/vulkan/rendering_device_driver_vulkan.cpp"
at = planned[path].index("#include ")
planned[path] = planned[path][:at] + INCLUDE + "\n" + planned[path][at:]
replace(path, '\tconst uint32_t max_queue_count_per_family = 1;\n\tstatic const float queue_priorities[max_queue_count_per_family] = {};',
        '\tuint32_t max_queue_count_per_family = 1;\n#ifdef MODULE_RESCALEFRAME_ENABLED\n'
        '\tif (RSFGodot::vulkan_enabled()) max_queue_count_per_family = 4;\n#endif\n'
        '\tstatic const float queue_priorities[4] = {};')
replace(path, '\tfor (uint32_t i = 0; i < queue_family.size(); i++) {\n\t\tif (queue_family[i].virtual_count',
        '\tfor (uint32_t i = 0; i < queue_family.size(); i++) {\n#ifdef MODULE_RESCALEFRAME_ENABLED\n'
        '\t\t// Queues 1..3 are reserved for the native presentation owner.\n'
        '\t\tif (RSFGodot::vulkan_enabled() && queue_family.size() >= 4 && i != 0) continue;\n#endif\n'
        '\t\tif (queue_family[i].virtual_count')
replace(path, '\t\t\twait_semaphores_stages.push_back(VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT);',
        '\t\t\tconst auto *acquired_chain = command_queue->image_semaphores_swap_chains[command_queue->pending_semaphores_for_execute[i]];\n'
        '\t\t\twait_semaphores_stages.push_back(acquired_chain && acquired_chain->rescaleframe_owner ? VK_PIPELINE_STAGE_ALL_COMMANDS_BIT : VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT);')
replace(path, '\t\tif (fence != nullptr && !command_queue->pending_semaphores_for_fence.is_empty()) {',
        '#ifdef MODULE_RESCALEFRAME_ENABLED\n'
        '\t\tRSFGodot::vk_submitted(reinterpret_cast<void *const *>(command_buffers.ptr()), command_buffers.size());\n#endif\n'
        '\t\tif (fence != nullptr && !command_queue->pending_semaphores_for_fence.is_empty()) {')
# Both platform branches must select the same WSI owner.
planned[path] = planned[path].replace('err = device_functions.QueuePresentKHR(device_queue.queue, &present_info);',
        'err = rescaleframe_present(p_cmd_queue, &present_info, p_swap_chains);')
if planned[path].count('err = rescaleframe_present(p_cmd_queue, &present_info, p_swap_chains);') != 2:
    raise SystemExit("Unexpected Godot Vulkan present branches")
replace(path, '\terr = device_functions.CreateSwapchainKHR(vk_device, &swap_create_info, VKC::get_allocation_callbacks(VK_OBJECT_TYPE_SWAPCHAIN_KHR), &swap_chain->vk_swapchain);',
        '#ifdef MODULE_RESCALEFRAME_ENABLED\n'
        '\tif (rescaleframe_create(p_cmd_queue, swap_chain, &swap_create_info, VKC::get_allocation_callbacks(VK_OBJECT_TYPE_SWAPCHAIN_KHR))) {\n'
        '\t\terr = VK_SUCCESS;\n\t} else\n#endif\n'
        '\t{\n\t\terr = device_functions.CreateSwapchainKHR(vk_device, &swap_create_info, VKC::get_allocation_callbacks(VK_OBJECT_TYPE_SWAPCHAIN_KHR), &swap_chain->vk_swapchain);\n\t}')
planned[path] = planned[path].replace('device_functions.GetSwapchainImagesKHR(vk_device, swap_chain->vk_swapchain, &image_count,',
        'rescaleframe_images(swap_chain, &image_count,')
replace(path, '\terr = device_functions.AcquireNextImageKHR(vk_device, swap_chain->vk_swapchain, UINT64_MAX, semaphore, VK_NULL_HANDLE, &swap_chain->image_index);',
        '\terr = rescaleframe_acquire(swap_chain, UINT64_MAX, semaphore, VK_NULL_HANDLE, &swap_chain->image_index);')
replace(path, 'void RenderingDeviceDriverVulkan::_swap_chain_release(SwapChain *swap_chain) {',
        'bool RenderingDeviceDriverVulkan::_swap_chain_release(SwapChain *swap_chain) {\n'
        '\t// Drain the complete proxy owner before retiring engine views and semaphores.\n'
        '\tif (!rescaleframe_release(swap_chain)) return false;')
replace(path, '\tswap_chain->present_semaphores.clear();\n}', '\tswap_chain->present_semaphores.clear();\n\treturn true;\n}')
replace(path, '\t// Release all current contents of the swap chain.\n\t_swap_chain_release(swap_chain);',
        '\t// Release all current contents of the swap chain.\n\tERR_FAIL_COND_V(!_swap_chain_release(swap_chain), FAILED);')
replace(path, '\t_swap_chain_release(swap_chain);\n\n\tmemdelete(swap_chain);',
        '\tERR_FAIL_COND(!_swap_chain_release(swap_chain));\n\n\tmemdelete(swap_chain);')
path = "drivers/vulkan/rendering_device_driver_vulkan.h"
replace(path, '\t\tBinaryMutex submit_mutex;', '\t\tMutex submit_mutex; // Native SDK submission callbacks may reenter on this thread.')
replace(path, '\tstruct SwapChain {', '\tstruct SwapChain {\n\t\tvoid *rescaleframe_owner = nullptr;\n'
        '\t\tRenderingDeviceDriverVulkan *rescaleframe_driver = nullptr;\n\t\tuint32_t rescaleframe_family = 0;')
replace(path, '\tvoid _swap_chain_release(SwapChain *p_swap_chain);', '\tbool _swap_chain_release(SwapChain *p_swap_chain);')
replace(path, '\tvoid *rescaleframe_begin(CommandBufferID id);',
        '\tbool rescaleframe_create(CommandQueueID queue, void *chain, const void *info, const void *allocator);\n'
        '\tbool rescaleframe_release(void *chain);\n'
        '\tVkResult rescaleframe_images(void *chain, uint32_t *count, VkImage *images);\n'
        '\tVkResult rescaleframe_acquire(void *chain, uint64_t timeout, VkSemaphore semaphore, VkFence fence, uint32_t *index);\n'
        '\tVkResult rescaleframe_present(CommandQueueID queue, const VkPresentInfoKHR *info, VectorView<SwapChainID> chains);\n'
        '\tstatic int32_t rescaleframe_submit(void *user, uint32_t count, const void *submits, uint64_t fence);\n'
        '\tstatic void rescaleframe_lock_queues(void *user);\n\tstatic void rescaleframe_unlock_queues(void *user);\n'
        '\tvoid *rescaleframe_begin(CommandBufferID id);')
previous_timeline_plan = planned.copy()
path = "drivers/vulkan/rendering_device_driver_vulkan.h"
replace(path, '\tstruct Queue {', '\tbool rescaleframe_timeline = false;\n\tstruct Queue {')
path = "drivers/vulkan/rendering_device_driver_vulkan.cpp"
replace(path, '\t\tfunctions.GetPhysicalDeviceFeatures2(physical_device, &device_features_2);',
        '\t\tfunctions.GetPhysicalDeviceFeatures2(physical_device, &device_features_2);\n'
        '#ifdef MODULE_RESCALEFRAME_ENABLED\n\t\trescaleframe_timeline = use_1_2_features && device_features_vk_1_2.timelineSemaphore;\n#endif')
replace(path, '\tvoid *create_info_next = nullptr;\n\tVkPhysicalDeviceShaderFloat16Int8FeaturesKHR shader_features = {};',
        '\tvoid *create_info_next = nullptr;\n'
        '\tVkPhysicalDeviceTimelineSemaphoreFeatures rsf_timeline = {};\n'
        '#ifdef MODULE_RESCALEFRAME_ENABLED\n\tif (RSFGodot::vulkan_enabled() && rescaleframe_timeline) {\n'
        '\t\trsf_timeline.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES;\n'
        '\t\trsf_timeline.timelineSemaphore = VK_TRUE;\n\t\tcreate_info_next = &rsf_timeline;\n\t}\n#endif\n'
        '\tVkPhysicalDeviceShaderFloat16Int8FeaturesKHR shader_features = {};')
previous_proxy_layout_plan = planned.copy()
replace("drivers/vulkan/rendering_device_driver_vulkan.cpp", '\tattachment.finalLayout = VK_IMAGE_LAYOUT_PRESENT_SRC_KHR;',
        '\t// The proxy exposes regular shader-readable images, not WSI images.\n'
        '\tattachment.finalLayout = swap_chain->rescaleframe_owner ? VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL : VK_IMAGE_LAYOUT_PRESENT_SRC_KHR;')
previous_multi_present_plan = planned.copy()
path = "drivers/vulkan/rendering_device_driver_vulkan.cpp"
replace(path, '\tif (!rescaleframe_release(swap_chain)) return false;', '\tif (!rescaleframe_drain(swap_chain)) return false;')
replace(path, '\t\tdevice_functions.DestroySwapchainKHR(vk_device, swap_chain->vk_swapchain, VKC::get_allocation_callbacks(VK_OBJECT_TYPE_SWAPCHAIN_KHR));',
        '\t\tif (swap_chain->rescaleframe_owner) {\n\t\t\tif (!rescaleframe_release(swap_chain)) return false;\n'
        '\t\t} else {\n\t\t\tdevice_functions.DestroySwapchainKHR(vk_device, swap_chain->vk_swapchain, VKC::get_allocation_callbacks(VK_OBJECT_TYPE_SWAPCHAIN_KHR));\n\t\t}')
planned[path] = planned[path].replace('rescaleframe_present(p_cmd_queue, &present_info, p_swap_chains);',
        'rescaleframe_present(p_cmd_queue, &present_info, p_swap_chains, p_cmd_buffers.size() > 0);')
path = "drivers/vulkan/rendering_device_driver_vulkan.h"
replace(path, '\tbool rescaleframe_release(void *chain);', '\tbool rescaleframe_release(void *chain);\n\tbool rescaleframe_drain(void *chain);')
replace(path, 'const VkPresentInfoKHR *info, VectorView<SwapChainID> chains);',
        'const VkPresentInfoKHR *info, VectorView<SwapChainID> chains, bool submitted);')
previous_latency_plan = planned.copy()
path = "drivers/vulkan/rendering_context_driver_vulkan.cpp"
at = planned[path].index("#include ")
planned[path] = planned[path][:at] + INCLUDE + "\n" + planned[path][at:]
replace(path, '\terr = _initialize_vulkan_version();',
        '#if defined(USE_VOLK) && defined(MODULE_RESCALEFRAME_ENABLED)\n'
        '\t// Streamline must observe instance/device creation and own all later dispatch lookups.\n'
        '\tif (uint64_t resolver = RSFGodot::vk_resolver()) volkInitializeCustom(reinterpret_cast<PFN_vkGetInstanceProcAddr>(resolver));\n'
        '#endif\n\n\terr = _initialize_vulkan_version();')
previous_latency_wsi_plan = planned.copy()
replace("drivers/vulkan/rendering_context_driver_vulkan.h", '\tstruct Surface {',
        '\tstruct Surface {\n\t\tuint64_t rescaleframe_window = 0;')
replace("platform/windows/rendering_context_driver_vulkan_windows.cpp", '\tsurface->vk_surface = vk_surface;',
        '\tsurface->vk_surface = vk_surface;\n\tsurface->rescaleframe_window = reinterpret_cast<uint64_t>(wpd->window);')
previous_latency_owner_plan = planned.copy()
replace("platform/windows/rendering_context_driver_vulkan_windows.h", '\t\tHINSTANCE instance;',
        '\t\tHINSTANCE instance;\n\t\tbool rescaleframe_primary;')
replace("platform/windows/display_server_windows.cpp", '\t\twpd.vulkan.window = wd.hWnd;',
        '\t\twpd.vulkan.window = wd.hWnd;\n\t\twpd.vulkan.rescaleframe_primary = p_window_id == DisplayServerEnums::MAIN_WINDOW_ID;')
replace("drivers/vulkan/rendering_context_driver_vulkan.h", '\t\tuint64_t rescaleframe_window = 0;',
        '\t\tuint64_t rescaleframe_window = 0;\n\t\tbool rescaleframe_primary = false;')
replace("platform/windows/rendering_context_driver_vulkan_windows.cpp", '\tsurface->rescaleframe_window = reinterpret_cast<uint64_t>(wpd->window);',
        '\tsurface->rescaleframe_window = reinterpret_cast<uint64_t>(wpd->window);\n\tsurface->rescaleframe_primary = wpd->rescaleframe_primary;')
replace("drivers/vulkan/rendering_device_driver_vulkan.cpp", '\tVkSwapchainCreateInfoKHR swap_create_info = {};',
        '\tVkSwapchainCreateInfoKHR swap_create_info = {};\n'
        '\tVkSwapchainLatencyCreateInfoNV rescaleframe_latency = {};\n'
        '#ifdef MODULE_RESCALEFRAME_ENABLED\n'
        '\tif (RSFGodot::vulkan_enabled() && context_driver->functions_get().GetDeviceProcAddr(vk_device, "vkLatencySleepNV")) {\n'
        '\t\trescaleframe_latency.sType = VK_STRUCTURE_TYPE_SWAPCHAIN_LATENCY_CREATE_INFO_NV;\n'
        '\t\trescaleframe_latency.latencyModeEnable = surface->rescaleframe_primary ? VK_TRUE : VK_FALSE;\n'
        '\t\tswap_create_info.pNext = &rescaleframe_latency;\n\t}\n#endif')
changed = set(subprocess.check_output(["git", "-C", str(ENGINE), "diff", "--name-only"], text=True).splitlines())
if changed - set(FILES):
    raise SystemExit("Unexpected reference changes: " + ", ".join(sorted(changed - set(FILES))))
for path in FILES:
    current = (ENGINE / path).read_text(encoding="utf-8")
    validated[path] = current
    if current not in (original[path], previous_plan[path], previous_camera_plan[path], previous_queue_plan[path], previous_pacing_plan[path], previous_vulkan_plan[path], previous_copy_usage_plan[path], previous_validation_plan[path], previous_capture_plan[path], previous_wsi_plan[path], previous_timeline_plan[path], previous_proxy_layout_plan[path], previous_multi_present_plan[path], previous_latency_plan[path], previous_latency_wsi_plan[path], previous_latency_owner_plan[path], planned[path]):
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
