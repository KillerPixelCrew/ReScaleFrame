add_library(rsf_game_gta5_enhanced SHARED src/plugin.cpp src/native_inputs.cpp src/native_renderer.cpp)
target_link_libraries(rsf_game_gta5_enhanced PRIVATE ReScaleFrame::GameSDK minhook d3d12 dxgi rsf_build_options)
set_target_properties(rsf_game_gta5_enhanced PROPERTIES OUTPUT_NAME "ReScaleFrame.Game.GTA5Enhanced")

# ASI entry only. An external, version-compatible ASI loader remains a game installation dependency.
add_library(rsf_asi_gta5_enhanced SHARED src/asi_entry.cpp)
target_include_directories(rsf_asi_gta5_enhanced PRIVATE ../../runtime/orchestrator/include)
target_link_libraries(rsf_asi_gta5_enhanced PRIVATE
    ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon ReScaleFrame::Contract rsf_build_options)
if(TARGET rsf_vendor_census)
    target_link_libraries(rsf_asi_gta5_enhanced PRIVATE ReScaleFrame::VendorCensus)
    target_compile_definitions(rsf_asi_gta5_enhanced PRIVATE RSF_HAVE_VENDOR_CENSUS)
endif()
target_compile_definitions(rsf_asi_gta5_enhanced PRIVATE _CRT_SECURE_NO_WARNINGS)
set_target_properties(rsf_asi_gta5_enhanced PROPERTIES
    OUTPUT_NAME "ReScaleFrame.GTA5Enhanced" PREFIX "" SUFFIX ".asi"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/gta5-enhanced/bin/$<CONFIG>"
    LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/gta5-enhanced/bin/$<CONFIG>"
    ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/gta5-enhanced/lib/$<CONFIG>"
    PDB_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/gta5-enhanced/bin/$<CONFIG>")
add_dependencies(rsf_asi_gta5_enhanced rsf_orchestrator rsf_game_gta5_enhanced)
