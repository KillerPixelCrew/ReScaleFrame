# Plugin policy (recognition, lifecycle, SR routing, completed-scene producer, camera decode) is the Rust crate
# rsf-rdr2, including the exported entry and the Windows renderer/input leaves.
# The F10 frame capture, probes, D3D12 trace, census, module dumps and RenderDoc frame probe are
# research tooling: developer builds only (the Rust `research` feature and RSF_RDR2_RESEARCH).
option(RSF_RDR2_RESEARCH "Build the RDR2 research capture and diagnostics into the ASI" OFF)
if(RSF_RDR2_RESEARCH)
    rsf_add_rust_static(rsf_rdr2_rust rsf_rust_rdr2 rsf-rdr2 rsf_rdr2 --features research)
else()
    rsf_add_rust_static(rsf_rdr2_rust rsf_rust_rdr2 rsf-rdr2 rsf_rdr2)
endif()
add_library(rsf_game_rdr2 SHARED src/plugin.def src/plugin.c)
set_target_properties(rsf_game_rdr2 PROPERTIES LINKER_LANGUAGE C)
if(MSVC)
    target_link_options(rsf_game_rdr2 PRIVATE "LINKER:/INCLUDE:rsf_get_game_plugin_api")
elseif(MINGW)
    target_link_options(rsf_game_rdr2 PRIVATE "LINKER:--undefined=rsf_get_game_plugin_api")
endif()
target_link_libraries(rsf_game_rdr2 PRIVATE rsf_rdr2_rust ReScaleFrame::GameSDK minhook user32 rsf_build_options)
set_target_properties(rsf_game_rdr2 PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.RDR2")

# Loaded by the game's ASI loader, so the file name is not a system DLL and nothing is forwarded.
# Startup admission, configuration gates, ForceD3D12 and engine-settings ownership are the same Rust
# archive. asi_entry.c provides the CRT entry and binds the ASI-only C services.
add_library(rsf_asi_rdr2 SHARED src/asi_entry.c src/social_club_guard.c)
target_include_directories(rsf_asi_rdr2 PRIVATE ../../runtime/orchestrator/include)
target_link_libraries(rsf_asi_rdr2 PRIVATE rsf_rdr2_rust
    ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon ReScaleFrame::Contract ReScaleFrame::LoaderDiagnostics
    ReScaleFrame::VendorCensus minhook dxgi rsf_build_options)
if(RSF_RDR2_RESEARCH)
    target_sources(rsf_asi_rdr2 PRIVATE src/capture_ngx.c)
    target_compile_definitions(rsf_asi_rdr2 PRIVATE RSF_RDR2_RESEARCH)
endif()
target_compile_definitions(rsf_asi_rdr2 PRIVATE _CRT_SECURE_NO_WARNINGS)
set_target_properties(rsf_asi_rdr2 PROPERTIES OUTPUT_NAME "ReScaleFrame.RDR2" PREFIX "" SUFFIX ".asi"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/rdr2/bin/$<CONFIG>"
    LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/rdr2/bin/$<CONFIG>"
    ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/rdr2/lib/$<CONFIG>"
    PDB_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/rdr2/bin/$<CONFIG>")
add_dependencies(rsf_asi_rdr2 rsf_orchestrator rsf_game_rdr2)
# The census and module dump libraries call rsf-loader, which the RDR2 archive carries.
set_target_properties(rsf_asi_rdr2 PROPERTIES RSF_EMBEDDED_ARCHIVES rsf_loader_rust)

# Explicit research helper, never autoloaded or included in the ASI deployment.
add_library(rsf_rdr2_attach SHARED EXCLUDE_FROM_ALL src/attach_entry.c)
target_include_directories(rsf_rdr2_attach PRIVATE ../../runtime/orchestrator/include)
target_compile_definitions(rsf_rdr2_attach PRIVATE _CRT_SECURE_NO_WARNINGS)
target_link_libraries(rsf_rdr2_attach PRIVATE rsf_rdr2_rust ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon ReScaleFrame::Contract minhook user32 rsf_build_options)
set_target_properties(rsf_rdr2_attach PROPERTIES OUTPUT_NAME "ReScaleFrame.RDR2.Attach")
