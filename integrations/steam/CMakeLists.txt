# The native exception guards. A Rust archive that embeds rsf-steam as an rlib (Skyrim
# SE) links only this; standalone consumers (Fallout 4) link the Steam archive below.
add_library(rsf_steam_native INTERFACE)
target_sources(rsf_steam_native INTERFACE "${CMAKE_CURRENT_SOURCE_DIR}/src/exception_guards.c")
target_include_directories(rsf_steam_native INTERFACE
    "${CMAKE_SOURCE_DIR}/runtime/orchestrator/include"
    "${CMAKE_SOURCE_DIR}/runtime/graphics/include"
    "${CMAKE_SOURCE_DIR}/runtime/presentation/include")
target_link_libraries(rsf_steam_native INTERFACE ReScaleFrame::GameSDK bcrypt minhook)
rsf_add_rust_static(rsf_steam_d3d11_chain rsf_rust_steam rsf-steam rsf_steam)
target_link_libraries(rsf_steam_d3d11_chain INTERFACE rsf_steam_native)
