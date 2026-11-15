add_library(rsf_game_skyrim_se SHARED src/plugin.cpp src/native_creation.cpp src/compatibility.cpp src/cpu_events.cpp src/camera.cpp src/render_inputs.cpp src/resolution.cpp src/auxiliary.cpp src/sampling.cpp src/presentation_inputs.cpp)
target_include_directories(rsf_game_skyrim_se PRIVATE
    "${CMAKE_SOURCE_DIR}/runtime/orchestrator/include" "${CMAKE_SOURCE_DIR}/runtime/graphics/include")
target_link_libraries(rsf_game_skyrim_se PRIVATE
    ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon minhook rsf_build_options)
set_target_properties(rsf_game_skyrim_se PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.SkyrimSE")
