add_library(rsf_steam_d3d11_chain STATIC src/d3d11_chain.cpp)
target_include_directories(rsf_steam_d3d11_chain PUBLIC include PRIVATE
    "${CMAKE_SOURCE_DIR}/runtime/orchestrator/include"
    "${CMAKE_SOURCE_DIR}/runtime/graphics/include"
    "${CMAKE_SOURCE_DIR}/runtime/presentation/include")
target_link_libraries(rsf_steam_d3d11_chain PRIVATE
    ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon minhook rsf_build_options)
