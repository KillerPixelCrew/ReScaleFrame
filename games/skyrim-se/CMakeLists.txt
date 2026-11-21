# rsf-skyrim-se owns the GameSDK table and lifecycle, exact-build observer admission, cold native
# creation and Steam Present cooperation. Native code lends the platform callbacks and typed detours.
rsf_add_rust_static(rsf_skyrim_se_rust rsf_rust_skyrim_se rsf-skyrim-se rsf_skyrim_se)
add_library(rsf_game_skyrim_se SHARED src/plugin.cpp src/native_creation.cpp src/compatibility.cpp src/cpu_events.cpp src/camera.cpp src/render_inputs.cpp src/resolution.cpp src/auxiliary.cpp src/sampling.cpp src/presentation_inputs.cpp)
target_include_directories(rsf_game_skyrim_se PRIVATE
    "${CMAKE_SOURCE_DIR}/runtime/orchestrator/include" "${CMAKE_SOURCE_DIR}/runtime/graphics/include"
    "${CMAKE_SOURCE_DIR}/runtime/presentation/include")
target_link_libraries(rsf_game_skyrim_se PRIVATE
    rsf_skyrim_se_rust ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon rsf_steam_d3d11_chain minhook rsf_build_options)
set_target_properties(rsf_game_skyrim_se PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.SkyrimSE")
