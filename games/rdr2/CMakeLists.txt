# Plugin policy (recognition, lifecycle, SR routing, completed-scene producer, camera decode) is the Rust crate
# rsf-rdr2. native_platform.cpp keeps the MinHook, D3D12/DXGI, Win32 input/memory and caller-capture leaves.
rsf_add_rust_static(rsf_rdr2_rust rsf_rust_rdr2 rsf-rdr2 rsf_rdr2)
add_library(rsf_game_rdr2 SHARED src/plugin.cpp src/native_platform.cpp)
target_link_libraries(rsf_game_rdr2 PRIVATE rsf_rdr2_rust ReScaleFrame::GameSDK minhook d3d12 rsf_build_options)
set_target_properties(rsf_game_rdr2 PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.RDR2")

# Loaded by the game's ASI loader, so the file name is not a system DLL and nothing is forwarded.
# Startup admission, configuration gates, ForceD3D12 and engine-settings ownership are the same Rust
# archive; asi_entry.cpp lends it the ASI's own leaf table (asi_platform.h) and keeps the runtime session worker.
add_library(rsf_asi_rdr2 SHARED src/asi_entry.cpp src/social_club.cpp src/camera_probe.cpp src/native_capture.cpp src/script_input.cpp src/native_settings.cpp)
target_include_directories(rsf_asi_rdr2 PRIVATE ../../runtime/orchestrator/include)
target_link_libraries(rsf_asi_rdr2 PRIVATE rsf_rdr2_rust
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
