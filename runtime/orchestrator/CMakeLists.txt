add_library(rsf_orchestrator SHARED src/runtime.cpp src/frame_assembly.cpp)
# The orchestrator is the one component that may know both sides: it owns vendor SDKs and it drives
# the plugins. Frame assembly is where those two meet.
target_link_libraries(rsf_orchestrator PRIVATE rsf_build_options)
# Public because the orchestrator's own header names a backend's frame structure.
target_link_libraries(rsf_orchestrator PUBLIC ReScaleFrame::DLSS)
target_compile_definitions(rsf_orchestrator PRIVATE RSF_RUNTIME_BUILD)
target_include_directories(rsf_orchestrator PUBLIC include)
set_target_properties(rsf_orchestrator PROPERTIES OUTPUT_NAME "ReScaleFrame.Runtime")
