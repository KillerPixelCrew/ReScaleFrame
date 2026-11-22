rsf_add_rust_static(rsf_steam_d3d11_chain rsf_rust_steam rsf-steam rsf_steam)
target_sources(rsf_steam_d3d11_chain INTERFACE "${CMAKE_CURRENT_SOURCE_DIR}/src/exception_guards.c")
target_include_directories(rsf_steam_d3d11_chain INTERFACE include
    "${CMAKE_SOURCE_DIR}/runtime/orchestrator/include"
    "${CMAKE_SOURCE_DIR}/runtime/graphics/include"
    "${CMAKE_SOURCE_DIR}/runtime/presentation/include")
target_link_libraries(rsf_steam_d3d11_chain INTERFACE ReScaleFrame::GameSDK bcrypt minhook)
