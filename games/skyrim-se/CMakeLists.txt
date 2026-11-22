# rsf-skyrim-se owns the GameSDK table and lifecycle, exact-build observer admission, cold native
# creation, Steam Present cooperation and typed D3D11 render leases. Native code supplies the
# remaining detours and the callback exception boundary.
rsf_add_rust_static(rsf_skyrim_se_rust rsf_rust_skyrim_se rsf-skyrim-se rsf_skyrim_se)
add_library(rsf_game_skyrim_se SHARED src/plugin.c src/render_detours.cpp src/native_creation.cpp src/compatibility.cpp src/cpu_events.cpp src/guarded_read.cpp)
target_include_directories(rsf_game_skyrim_se PRIVATE
    "${CMAKE_SOURCE_DIR}/runtime/orchestrator/include" "${CMAKE_SOURCE_DIR}/runtime/graphics/include"
    "${CMAKE_SOURCE_DIR}/runtime/presentation/include")
target_link_libraries(rsf_game_skyrim_se PRIVATE
    rsf_skyrim_se_rust ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon rsf_steam_d3d11_chain minhook rsf_build_options)
set_target_properties(rsf_game_skyrim_se PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON OUTPUT_NAME "ReScaleFrame.Game.SkyrimSE")
