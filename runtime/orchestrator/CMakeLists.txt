# The orchestrator's code lives in an object library so it can be linked both into the runtime DLL
# and directly into the research proxy, which is what actually gets loaded into the game today.
# Compiling it twice, or requiring the runtime DLL beside the game executable before anything works,
# would both be worse.
add_library(rsf_orchestrator_core OBJECT
    src/runtime.cpp
    src/plugin_session.cpp
    src/native_scene.cpp
    src/native_window.cpp
    src/native_cpu.cpp
    src/native_fg.cpp
    src/native_fg_d3d12.cpp
    src/native_composition.cpp
    src/native_sr.cpp
    src/native_translucency.cpp
    src/native_sr_d3d12.cpp
    src/gpu_policy.cpp
    src/unity_sr_host.cpp
    src/overlay_d3d12.cpp
    ../../loader/proxy/src/overlay_host.cpp
    src/native_regions.cpp
    src/frame_sequencer.cpp
    src/render_links.cpp
    src/fg_session.cpp
    src/fg_choice.cpp
    src/fg_providers.cpp
    src/fg_leases.cpp
    src/sr_session.cpp
    src/sr_bridge.cpp
    src/sr_legacy_adapter.cpp
    src/frame_assembly.cpp
    src/dlss_pipeline.cpp)
target_include_directories(rsf_orchestrator_core PUBLIC include)
target_compile_definitions(rsf_orchestrator_core PUBLIC RSF_RUNTIME_BUILD)
target_compile_definitions(rsf_orchestrator_core PRIVATE RSF_STREAMLINE_HOST_BUILD)
# Public because the orchestrator's own headers name a backend's frame structure and the decode
# pass's parameters.
target_link_libraries(rsf_orchestrator_core PUBLIC ReScaleFrame::DLSS ReScaleFrame::Graphics ReScaleFrame::FSR ReScaleFrame::XeSS ReScaleFrame::Presentation)
target_link_libraries(rsf_orchestrator_core PUBLIC ReScaleFrame::GameSDK PRIVATE rsf_build_options)
target_link_libraries(rsf_orchestrator_core PRIVATE d3dcompiler bcrypt)
add_library(ReScaleFrame::OrchestratorCore ALIAS rsf_orchestrator_core)

add_library(rsf_orchestrator SHARED $<TARGET_OBJECTS:rsf_orchestrator_core>)
target_link_libraries(rsf_orchestrator PUBLIC rsf_orchestrator_core)
target_link_libraries(rsf_orchestrator PRIVATE rsf_build_options)
set_target_properties(rsf_orchestrator PROPERTIES OUTPUT_NAME "ReScaleFrame.Runtime")
