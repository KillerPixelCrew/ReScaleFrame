# rsf-skyrim-se owns the GameSDK table and lifecycle, exact-build observer admission, cold native
# creation, Steam Present cooperation and typed D3D11 render leases. Native code supplies the
# remaining detours and the callback exception boundary.
rsf_add_rust_static(rsf_skyrim_se_rust rsf_rust_skyrim_se rsf-skyrim-se rsf_skyrim_se)
# The native leaves the Rust archive calls: detours, guarded reads and exception boundaries. The
# adapter DLL and the input fixture link the same library.
add_library(rsf_skyrim_se_native STATIC src/render_detours.c src/native_creation.c src/compatibility.c
    src/cpu_events.c src/guarded_read.c)
target_include_directories(rsf_skyrim_se_native PUBLIC
    "${CMAKE_SOURCE_DIR}/runtime/orchestrator/include" "${CMAKE_SOURCE_DIR}/runtime/graphics/include"
    "${CMAKE_SOURCE_DIR}/runtime/presentation/include")
target_link_libraries(rsf_skyrim_se_native PUBLIC ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon
    rsf_steam_native minhook PRIVATE rsf_build_options)
set_target_properties(rsf_skyrim_se_native PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON)
target_link_libraries(rsf_skyrim_se_rust INTERFACE rsf_skyrim_se_native)
add_library(rsf_game_skyrim_se SHARED src/plugin.c)
target_link_libraries(rsf_game_skyrim_se PRIVATE rsf_skyrim_se_rust rsf_build_options)
set_target_properties(rsf_game_skyrim_se PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON OUTPUT_NAME "ReScaleFrame.Game.SkyrimSE")
