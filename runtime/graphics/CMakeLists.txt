add_library(rsf_graphics STATIC
    src/texture_dump.cpp
    src/motion_decode.cpp
    src/d3d11_observer.cpp)
target_include_directories(rsf_graphics PUBLIC include)
target_link_libraries(rsf_graphics PRIVATE rsf_build_options)
target_link_libraries(rsf_graphics PUBLIC d3d11 dxgi)
add_library(ReScaleFrame::Graphics ALIAS rsf_graphics)
