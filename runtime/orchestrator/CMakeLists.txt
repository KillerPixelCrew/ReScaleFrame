# The orchestrator's code lives in an object library so it can be linked both into the runtime DLL
# and directly into the research proxy, which is what actually gets loaded into the game today.
# Compiling it twice, or requiring the runtime DLL beside the game executable before anything works,
# would both be worse.
add_library(rsf_orchestrator_core OBJECT
    src/runtime.cpp
    src/frame_assembly.cpp
    src/dlss_pipeline.cpp)
target_include_directories(rsf_orchestrator_core PUBLIC include)
target_compile_definitions(rsf_orchestrator_core PUBLIC RSF_RUNTIME_BUILD)
# Public because the orchestrator's own headers name a backend's frame structure and the decode
# pass's parameters.
target_link_libraries(rsf_orchestrator_core PUBLIC ReScaleFrame::DLSS ReScaleFrame::Graphics)
target_link_libraries(rsf_orchestrator_core PRIVATE rsf_build_options)
add_library(ReScaleFrame::OrchestratorCore ALIAS rsf_orchestrator_core)

add_library(rsf_orchestrator SHARED $<TARGET_OBJECTS:rsf_orchestrator_core>)
target_link_libraries(rsf_orchestrator PUBLIC rsf_orchestrator_core)
target_link_libraries(rsf_orchestrator PRIVATE rsf_build_options)
set_target_properties(rsf_orchestrator PROPERTIES OUTPUT_NAME "ReScaleFrame.Runtime")
