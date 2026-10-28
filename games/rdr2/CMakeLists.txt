add_library(rsf_game_rdr2 SHARED src/plugin.cpp src/render_camera.cpp src/native_renderer.cpp src/scene_boundary.cpp)
target_link_libraries(rsf_game_rdr2 PRIVATE ReScaleFrame::GameSDK minhook d3d12 rsf_build_options)
set_target_properties(rsf_game_rdr2 PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.RDR2")

# Loaded by the game's ASI loader, so the file name is not a system DLL and nothing is forwarded.
add_library(rsf_asi_rdr2 SHARED src/asi_entry.cpp src/social_club.cpp src/camera_probe.cpp src/native_capture.cpp src/script_input.cpp src/native_settings.cpp src/engine_settings.cpp)
target_include_directories(rsf_asi_rdr2 PRIVATE ../../runtime/orchestrator/include)
target_link_libraries(rsf_asi_rdr2 PRIVATE
    ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon ReScaleFrame::Contract ReScaleFrame::LoaderDiagnostics
    ReScaleFrame::VendorCensus minhook dxgi rsf_build_options)
target_compile_definitions(rsf_asi_rdr2 PRIVATE _CRT_SECURE_NO_WARNINGS)
set_target_properties(rsf_asi_rdr2 PROPERTIES OUTPUT_NAME "ReScaleFrame.RDR2" PREFIX "" SUFFIX ".asi"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/rdr2/bin/$<CONFIG>"
    LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/rdr2/bin/$<CONFIG>"
    ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/rdr2/lib/$<CONFIG>"
    PDB_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/rdr2/bin/$<CONFIG>")
add_dependencies(rsf_asi_rdr2 rsf_orchestrator rsf_game_rdr2)

# Explicit research helper, never autoloaded or included in the ASI deployment.
add_library(rsf_rdr2_attach SHARED src/attach_entry.cpp)
target_include_directories(rsf_rdr2_attach PRIVATE ../../runtime/orchestrator/include)
target_compile_definitions(rsf_rdr2_attach PRIVATE _CRT_SECURE_NO_WARNINGS)
target_link_libraries(rsf_rdr2_attach PRIVATE ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon ReScaleFrame::Contract rsf_build_options)
set_target_properties(rsf_rdr2_attach PROPERTIES OUTPUT_NAME "ReScaleFrame.RDR2.Attach")
