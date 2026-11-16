add_library(rsf_game_fallout4 SHARED src/plugin.cpp src/native_creation.cpp
    src/cpu_events.cpp src/camera.cpp src/render_inputs.cpp src/render_hooks.cpp src/resolution.cpp src/screen.cpp
    src/depth_domains.cpp)
target_include_directories(rsf_game_fallout4 PRIVATE
    "${CMAKE_SOURCE_DIR}/runtime/orchestrator/include"
    "${CMAKE_SOURCE_DIR}/runtime/graphics/include"
    "${CMAKE_SOURCE_DIR}/runtime/presentation/include")
target_link_libraries(rsf_game_fallout4 PRIVATE ReScaleFrame::GameSDK
    ReScaleFrame::RuntimeCommon rsf_steam_d3d11_chain minhook rsf_build_options)
set_target_properties(rsf_game_fallout4 PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.Fallout4")
