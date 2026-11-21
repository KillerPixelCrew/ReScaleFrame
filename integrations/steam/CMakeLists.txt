# The Steam overlay chain owner is Rust (src/lib.rs). src/d3d11_chain.cpp keeps the adapters' C++
# entry point and supplies MinHook, exception guards and the shared file hash.
add_library(rsf_steam_d3d11_chain STATIC src/d3d11_chain.cpp)
rsf_add_rust_static(rsf_steam_rust rsf_rust_steam rsf-steam rsf_steam)
target_include_directories(rsf_steam_d3d11_chain PUBLIC include PRIVATE
    "${CMAKE_SOURCE_DIR}/runtime/orchestrator/include"
    "${CMAKE_SOURCE_DIR}/runtime/graphics/include"
    "${CMAKE_SOURCE_DIR}/runtime/presentation/include")
target_link_libraries(rsf_steam_d3d11_chain PRIVATE
    rsf_steam_rust ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon minhook rsf_build_options)
