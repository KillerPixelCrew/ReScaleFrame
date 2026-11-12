# SR Stage 1 round-trip shader: the RSF-authored HLSL is embedded as a string and compiled at runtime with the system
# d3dcompiler_47 (rescaleframe/shader_compile.h). The adapter and its D3D11 fixture include the same generated header.
set(RSF_ANOMALY_SR_SHADER "${CMAKE_CURRENT_SOURCE_DIR}/shaders/sr/rsf_sr_roundtrip.hlsl")
set(RSF_ANOMALY_SR_GENERATED "${CMAKE_CURRENT_BINARY_DIR}/generated")
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${RSF_ANOMALY_SR_SHADER}")
file(READ "${RSF_ANOMALY_SR_SHADER}" RSF_ANOMALY_SR_SHADER_TEXT)
file(CONFIGURE OUTPUT "${RSF_ANOMALY_SR_GENERATED}/sr_roundtrip_hlsl.h"
    CONTENT "// Generated from games/anomaly/shaders/sr/rsf_sr_roundtrip.hlsl. Do not edit.\n#pragma once\ninline constexpr char rsf_sr_roundtrip_hlsl[] = R\"rsf_hlsl(@RSF_ANOMALY_SR_SHADER_TEXT@)rsf_hlsl\";\n"
    @ONLY NEWLINE_STYLE LF)
add_library(rsf_anomaly_sr_shaders INTERFACE)
target_include_directories(rsf_anomaly_sr_shaders INTERFACE "${RSF_ANOMALY_SR_GENERATED}")

add_library(rsf_game_anomaly SHARED src/plugin.cpp src/bootstrap.cpp src/native_observer.cpp src/native_hooks.cpp
    src/native_geometry.cpp src/geometry_feed.cpp
    src/overlay_input_gate.cpp src/overlay_hooks.cpp src/overlay_service.cpp
    src/sr_swap.cpp src/sr_d3d11.cpp src/sr_stage.cpp)
target_include_directories(rsf_game_anomaly PRIVATE "${PROJECT_SOURCE_DIR}/runtime/orchestrator/include")
target_link_libraries(rsf_game_anomaly PRIVATE ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::RuntimeCommon minhook rsf_build_options)
# Stage 1 uses only the generic immediate-context state scope (d3d11_state) and the texture-dump view enum header.
target_link_libraries(rsf_game_anomaly PRIVATE ReScaleFrame::Graphics rsf_anomaly_sr_shaders d3d11)
set_target_properties(rsf_game_anomaly PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.Anomaly")
