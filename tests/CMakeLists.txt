add_library(rsf_sdk_c_header OBJECT sdk_c_header.c)
target_link_libraries(rsf_sdk_c_header PRIVATE ReScaleFrame::GameSDK rsf_build_options)

add_executable(rsf_plugin_contract plugin_contract.cpp)
target_link_libraries(rsf_plugin_contract PRIVATE ReScaleFrame::GameSDK rsf_build_options)
add_dependencies(rsf_plugin_contract rsf_game_ac7 rsf_game_project_wingman rsf_game_unity_mono)
target_compile_definitions(rsf_plugin_contract PRIVATE _CRT_SECURE_NO_WARNINGS)
add_executable(rsf_unity_sr_d3d12_test unity_sr_d3d12.cpp)
target_include_directories(rsf_unity_sr_d3d12_test PRIVATE ../games/unity-mono/include)
target_link_libraries(rsf_unity_sr_d3d12_test PRIVATE rsf_orchestrator rsf_build_options d3d12 dxgi)
if(EXISTS "${RSF_STREAMLINE_INCLUDE}/sl.h")
    target_include_directories(rsf_unity_sr_d3d12_test SYSTEM PRIVATE "${RSF_STREAMLINE_INCLUDE}")
endif()
add_test(NAME unity_sr_d3d12_contract COMMAND rsf_unity_sr_d3d12_test)
add_executable(rsf_gpu_policy_test gpu_policy.cpp)
target_link_libraries(rsf_gpu_policy_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME gpu_sr_policy COMMAND rsf_gpu_policy_test)
add_executable(rsf_unity_mono_runtime_test unity_mono_runtime.cpp)
target_link_libraries(rsf_unity_mono_runtime_test PRIVATE rsf_build_options)
if(MSVC)
    add_executable(rsf_shared_shim_exports shared_shim_exports.cpp)
    target_link_libraries(rsf_shared_shim_exports PRIVATE rsf_build_options)
    add_dependencies(rsf_shared_shim_exports rsf_proxy_dinput8)
    add_test(NAME shared_shim_exports COMMAND rsf_shared_shim_exports "$<TARGET_FILE:rsf_proxy_dinput8>")
endif()
if(NOT MSVC)
    # This test uses wmain, which mingw only selects as the entry point with -municode.
    target_compile_options(rsf_plugin_contract PRIVATE -municode)
    target_link_options(rsf_plugin_contract PRIVATE -municode)
endif()
# Load the plugin by name from its own directory. That is how it is loaded in practice, and it
# keeps the test runnable under a cross-build emulator, which cannot take a host path.
add_test(NAME game_plugin_contract COMMAND rsf_plugin_contract "$<TARGET_FILE_NAME:rsf_game_ac7>" ac7)
set_tests_properties(game_plugin_contract PROPERTIES
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_game_ac7>")
add_test(NAME game_project_wingman_plugin_contract COMMAND rsf_plugin_contract
    "$<TARGET_FILE_NAME:rsf_game_project_wingman>" project-wingman)
set_tests_properties(game_project_wingman_plugin_contract PROPERTIES
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_game_project_wingman>")
add_test(NAME game_unity_mono_plugin_contract COMMAND rsf_plugin_contract
    "$<TARGET_FILE_NAME:rsf_game_unity_mono>" unity-mono)
set_tests_properties(game_unity_mono_plugin_contract PROPERTIES
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_game_unity_mono>")
add_executable(rsf_module_dump_entropy module_dump_entropy.c)
target_link_libraries(rsf_module_dump_entropy PRIVATE ReScaleFrame::LoaderDiagnostics rsf_build_options)
add_test(NAME module_dump_entropy COMMAND rsf_module_dump_entropy)

add_executable(rsf_module_dump_self module_dump_self.c)
target_link_libraries(rsf_module_dump_self PRIVATE ReScaleFrame::LoaderDiagnostics rsf_build_options)
target_compile_definitions(rsf_module_dump_self PRIVATE _CRT_SECURE_NO_WARNINGS)
add_test(NAME module_dump_self COMMAND rsf_module_dump_self "${CMAKE_CURRENT_BINARY_DIR}")

add_executable(rsf_texture_dump texture_dump.cpp)
target_link_libraries(rsf_texture_dump PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME texture_dump COMMAND rsf_texture_dump "${CMAKE_CURRENT_BINARY_DIR}")

add_executable(rsf_frame_assembly frame_assembly.cpp)
target_link_libraries(rsf_frame_assembly PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME frame_assembly COMMAND rsf_frame_assembly)

add_executable(rsf_ac7_view_test ac7_view.cpp)
target_link_libraries(rsf_ac7_view_test PRIVATE ReScaleFrame::AC7View rsf_build_options)
add_test(NAME ac7_view COMMAND rsf_ac7_view_test)

add_executable(rsf_ac7_motion_capture_test ac7_motion_capture.cpp)
target_link_libraries(rsf_ac7_motion_capture_test PRIVATE ReScaleFrame::AC7MotionCapture rsf_build_options)
add_test(NAME ac7_motion_capture COMMAND rsf_ac7_motion_capture_test)

add_executable(rsf_ac7_capture_io ac7_capture_io.cpp)
target_link_libraries(rsf_ac7_capture_io PRIVATE ReScaleFrame::AC7MotionCapture ReScaleFrame::Graphics rsf_build_options d3dcompiler)
add_test(NAME ac7_capture_io COMMAND rsf_ac7_capture_io)
set_tests_properties(ac7_capture_io PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_ac7_session ac7_session.cpp ../loader/proxy/src/preferences.c)
target_link_libraries(rsf_ac7_session PRIVATE ReScaleFrame::AC7SceneColor rsf_build_options)
add_test(NAME ac7_session COMMAND rsf_ac7_session)

add_executable(rsf_constant_override constant_override.cpp)
target_link_libraries(rsf_constant_override PRIVATE ReScaleFrame::Graphics rsf_build_options d3dcompiler)
add_test(NAME constant_override COMMAND rsf_constant_override)
set_tests_properties(constant_override PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

# No device: the classifier is a pure function over shadowed facts, which is the point of it.
add_executable(rsf_ac7_ui_rules_test ac7_ui_rules.cpp)
target_link_libraries(rsf_ac7_ui_rules_test PRIVATE ReScaleFrame::AC7UIRules rsf_build_options)
add_test(NAME ac7_ui_rules COMMAND rsf_ac7_ui_rules_test)

# No device: this holds answers decided elsewhere, so stand-in pointers exercise all of it.
add_executable(rsf_ui_identify ui_identify.cpp)
target_link_libraries(rsf_ui_identify PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME ui_identify COMMAND rsf_ui_identify)

# A vendor that is not compiled in still has to answer, and has to answer differently from one
# whose runtime is missing. Runs in both configurations and CI builds it both ways.
add_executable(rsf_fsr_backend fsr_backend.cpp)
target_link_libraries(rsf_fsr_backend PRIVATE ReScaleFrame::FSR ReScaleFrame::XeSS rsf_build_options)
add_test(NAME fsr_backend COMMAND rsf_fsr_backend)

# Choosing between three vendors on a machine that has none of them, which is the point of
# negotiation being a pure function over what each one said it could do.
add_executable(rsf_negotiate negotiate.cpp)
target_link_libraries(rsf_negotiate PRIVATE ReScaleFrame::Contract rsf_build_options)
add_test(NAME negotiate COMMAND rsf_negotiate)

# Whether a D3D11 texture can be opened by a D3D12 device here at all. Skips with 77 and the
# HRESULTs when the two runtimes disagree, which under Wine is a fact about the environment.
add_executable(rsf_shared_surface shared_surface.cpp)
target_link_libraries(rsf_shared_surface PRIVATE ReScaleFrame::Presentation rsf_build_options)
add_test(NAME shared_surface COMMAND rsf_shared_surface)
set_tests_properties(shared_surface PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

# The layer, and the alpha accumulation that decides whether diverting into it works at all.
add_executable(rsf_ui_layer ui_layer.cpp)
target_link_libraries(rsf_ui_layer PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME ui_layer COMMAND rsf_ui_layer)
set_tests_properties(ui_layer PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

# The composite's arithmetic, which nothing covered while it lived in present_blit.
add_executable(rsf_fullscreen_pass fullscreen_pass.cpp)
target_link_libraries(rsf_fullscreen_pass PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME fullscreen_pass COMMAND rsf_fullscreen_pass)
set_tests_properties(fullscreen_pass PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_resource_roles resource_roles.cpp)
target_link_libraries(rsf_resource_roles PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME resource_roles COMMAND rsf_resource_roles)

add_executable(rsf_motion_decode motion_decode.cpp)
target_link_libraries(rsf_motion_decode PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME motion_decode COMMAND rsf_motion_decode)
set_tests_properties(motion_decode PROPERTIES TIMEOUT 120)

add_executable(rsf_frame_tap frame_tap.cpp)
target_link_libraries(rsf_frame_tap PRIVATE ReScaleFrame::AC7SceneColor ReScaleFrame::Graphics rsf_build_options)
add_test(NAME frame_tap COMMAND rsf_frame_tap)
set_tests_properties(frame_tap PROPERTIES TIMEOUT 120)

add_executable(rsf_d3d11_state d3d11_state.cpp)
target_link_libraries(rsf_d3d11_state PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME d3d11_state COMMAND rsf_d3d11_state)
set_tests_properties(d3d11_state PROPERTIES TIMEOUT 120)

add_executable(rsf_scene_promote scene_promote.cpp)
target_link_libraries(rsf_scene_promote PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME scene_promote COMMAND rsf_scene_promote)
set_tests_properties(scene_promote PROPERTIES TIMEOUT 120)

add_executable(rsf_d3d11_observer d3d11_observer.cpp)
target_link_libraries(rsf_d3d11_observer PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME d3d11_observer COMMAND rsf_d3d11_observer "${CMAKE_CURRENT_BINARY_DIR}")
# These touch a graphics runtime and patch vtables, so a deadlock is a plausible failure. Fail in
# a couple of minutes rather than hanging the suite.
set_tests_properties(d3d11_observer texture_dump PROPERTIES TIMEOUT 120)

add_library(rsf_overlay_panel_fixture SHARED overlay_panel_fixture.cpp)
target_link_libraries(rsf_overlay_panel_fixture PRIVATE ReScaleFrame::Graphics rsf_build_options)
# Exported by property rather than by dllexport on each definition: the header declares the
# functions plainly, and MSVC refuses a definition whose linkage differs from its declaration.
set_target_properties(rsf_overlay_panel_fixture PROPERTIES PREFIX "" WINDOWS_EXPORT_ALL_SYMBOLS ON)
add_executable(rsf_overlay_host overlay_host.cpp ../loader/proxy/src/overlay_host.cpp)
target_link_libraries(rsf_overlay_host PRIVATE ReScaleFrame::Graphics rsf_build_options)
target_compile_definitions(rsf_overlay_host PRIVATE _CRT_SECURE_NO_WARNINGS)
add_dependencies(rsf_overlay_host rsf_overlay_panel_fixture)
add_test(NAME overlay_host COMMAND rsf_overlay_host "$<TARGET_FILE_NAME:rsf_overlay_panel_fixture>")
set_tests_properties(overlay_host PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_overlay_host>")

add_executable(rsf_dlss_backend dlss_backend.cpp)
target_link_libraries(rsf_dlss_backend PRIVATE ReScaleFrame::DLSS rsf_build_options)
add_test(NAME dlss_backend COMMAND rsf_dlss_backend)
# Loading a vendor runtime that talks to a driver is another place a hang is plausible.
set_tests_properties(dlss_backend PROPERTIES TIMEOUT 120)

add_test(NAME launcher_version COMMAND rsf_launcher --version)
set_tests_properties(launcher_version PROPERTIES PASS_REGULAR_EXPRESSION "ReScaleFrame ${PROJECT_VERSION}")

add_executable(rsf_sr_bridge_test sr_bridge.cpp)
target_link_libraries(rsf_sr_bridge_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME sr_bridge COMMAND rsf_sr_bridge_test)
set_tests_properties(sr_bridge PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)

# Link controller against deterministic providers, with no vendor DLLs or graphics device.
add_executable(rsf_sr_session_test sr_session.cpp ../runtime/orchestrator/src/sr_session.cpp)
target_compile_definitions(rsf_sr_session_test PRIVATE RSF_RUNTIME_BUILD)
target_include_directories(rsf_sr_session_test PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_sr_session_test PRIVATE ReScaleFrame::Contract rsf_build_options)
add_test(NAME sr_session COMMAND rsf_sr_session_test)

add_executable(rsf_motion_resolve_test motion_resolve.cpp)
target_link_libraries(rsf_motion_resolve_test PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME motion_resolve COMMAND rsf_motion_resolve_test)
set_tests_properties(motion_resolve PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)

add_executable(rsf_colour_transport_test colour_transport.cpp)
target_link_libraries(rsf_colour_transport_test PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME colour_transport COMMAND rsf_colour_transport_test)
set_tests_properties(colour_transport PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)
add_executable(rsf_native_translucency_test native_translucency.cpp)
target_link_libraries(rsf_native_translucency_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME native_translucency COMMAND rsf_native_translucency_test)
set_tests_properties(native_translucency PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)
add_executable(rsf_sr_pipeline_test sr_pipeline.cpp)
target_link_libraries(rsf_sr_pipeline_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME sr_pipeline COMMAND rsf_sr_pipeline_test)
set_tests_properties(sr_pipeline PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)

add_executable(rsf_fg_session_test fg_session.cpp ../runtime/orchestrator/src/fg_session.cpp
    ../runtime/orchestrator/src/frame_sequencer.cpp)
target_compile_definitions(rsf_fg_session_test PRIVATE RSF_RUNTIME_BUILD)
target_include_directories(rsf_fg_session_test PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_fg_session_test PRIVATE ReScaleFrame::Contract rsf_build_options d3d12)
add_test(NAME fg_session COMMAND rsf_fg_session_test)
set_tests_properties(fg_session PROPERTIES TIMEOUT 120)

add_executable(rsf_fg_provider_test fg_provider.cpp)
add_executable(rsf_fg_choice_test fg_choice.cpp)
target_link_libraries(rsf_fg_choice_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME fg_choice COMMAND rsf_fg_choice_test)
target_link_libraries(rsf_fg_provider_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME fg_provider COMMAND rsf_fg_provider_test)
set_tests_properties(fg_provider PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_fg_leases_test fg_leases.cpp)
target_link_libraries(rsf_fg_leases_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME fg_leases COMMAND rsf_fg_leases_test)
set_tests_properties(fg_leases PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_library(rsf_fg_c_header OBJECT fg_c_header.c)
target_include_directories(rsf_fg_c_header PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_fg_c_header PRIVATE ReScaleFrame::DLSS rsf_build_options)

add_executable(rsf_fg_switching_test fg_switching.cpp)
add_executable(rsf_d3d11_present_bridge_test d3d11_present_bridge.cpp)
add_executable(rsf_fg_present_bridge_test fg_present_bridge.cpp)
target_link_libraries(rsf_fg_present_bridge_test PRIVATE rsf_orchestrator_core rsf_build_options)
set_target_properties(rsf_fg_present_bridge_test PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/fixtures/fg/$<CONFIG>")
add_test(NAME shared_fg_present_bridge COMMAND rsf_fg_present_bridge_test)
set_tests_properties(shared_fg_present_bridge PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)
target_compile_definitions(rsf_d3d11_present_bridge_test PRIVATE RSF_STREAMLINE_HOST_BUILD)
target_link_libraries(rsf_d3d11_present_bridge_test PRIVATE rsf_orchestrator_core ReScaleFrame::AC7RenderScope rsf_build_options)
# Graphics fixtures must not load game-facing proxy DLLs from the shared artifact directory.
set_target_properties(rsf_d3d11_present_bridge_test PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/fixtures/fg/$<CONFIG>")
add_test(NAME d3d11_present_bridge COMMAND rsf_d3d11_present_bridge_test)
set_tests_properties(d3d11_present_bridge PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)
add_dependencies(rsf_d3d11_present_bridge_test rsf_proxy_ac7)
add_test(NAME ac7_proxy_startup COMMAND rsf_d3d11_present_bridge_test
    --proxy-smoke $<TARGET_FILE:rsf_proxy_ac7>)
set_tests_properties(ac7_proxy_startup PROPERTIES TIMEOUT 120)
target_link_libraries(rsf_fg_switching_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME fg_switching COMMAND rsf_fg_switching_test)
set_tests_properties(fg_switching PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_latency_host_test latency_host.cpp)
target_link_libraries(rsf_latency_host_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME latency_host COMMAND rsf_latency_host_test)
set_tests_properties(latency_host PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_render_links_test render_links.cpp ../runtime/orchestrator/src/render_links.cpp)
target_compile_definitions(rsf_render_links_test PRIVATE RSF_RUNTIME_BUILD)
target_include_directories(rsf_render_links_test PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_render_links_test PRIVATE ReScaleFrame::Contract rsf_build_options)
add_test(NAME render_links COMMAND rsf_render_links_test)
set_tests_properties(render_links PROPERTIES TIMEOUT 120)

add_executable(rsf_ac7_render_scope_test ac7_render_scope.cpp)
target_link_libraries(rsf_ac7_render_scope_test PRIVATE ReScaleFrame::AC7RenderScope rsf_build_options)
add_test(NAME ac7_render_scope COMMAND rsf_ac7_render_scope_test)
set_tests_properties(ac7_render_scope PROPERTIES TIMEOUT 120)

# Explicit hardware experiment, never run by routine CTest verification.
add_executable(rsf_exposure_probe ../tools/exposure-probe.cpp)
target_link_libraries(rsf_exposure_probe PRIVATE ReScaleFrame::DLSS ReScaleFrame::Graphics minhook rsf_build_options)
if(EXISTS "${RSF_RENDERDOC_INCLUDE}/renderdoc_app.h")
    target_include_directories(rsf_exposure_probe SYSTEM PRIVATE "${RSF_RENDERDOC_INCLUDE}")
    target_compile_definitions(rsf_exposure_probe PRIVATE RSF_PROBE_RENDERDOC=1)
endif()
set(RSF_NGX_PROBE_INCLUDE "" CACHE PATH "Optional NGX SDK headers for standalone probe parameter tracing")
if(EXISTS "${RSF_NGX_PROBE_INCLUDE}/nvsdk_ngx_params.h")
    target_include_directories(rsf_exposure_probe SYSTEM PRIVATE "${RSF_NGX_PROBE_INCLUDE}")
    target_compile_definitions(rsf_exposure_probe PRIVATE RSF_PROBE_NGX_PARAMS=1)
endif()

add_executable(rsf_ac7_native_graph_test ac7_native_graph.cpp)
target_link_libraries(rsf_ac7_native_graph_test PRIVATE ReScaleFrame::AC7RenderScope rsf_orchestrator minhook rsf_build_options)
add_test(NAME ac7_native_graph COMMAND rsf_ac7_native_graph_test)
set_tests_properties(ac7_native_graph PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)
