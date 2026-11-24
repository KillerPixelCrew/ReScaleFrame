# The native exception guards. The Rust archives that embed rsf-steam as an rlib (Skyrim SE,
# Fallout 4) link only this.
add_library(rsf_steam_native INTERFACE)
target_sources(rsf_steam_native INTERFACE "${CMAKE_CURRENT_SOURCE_DIR}/src/exception_guards.c")
target_include_directories(rsf_steam_native INTERFACE
    "${CMAKE_SOURCE_DIR}/runtime/orchestrator/include"
    "${CMAKE_SOURCE_DIR}/runtime/graphics/include"
    "${CMAKE_SOURCE_DIR}/runtime/presentation/include")
target_link_libraries(rsf_steam_native INTERFACE ReScaleFrame::GameSDK bcrypt minhook)
