add_library(rsf_graphics STATIC
    src/texture_dump.cpp
    src/motion_decode.cpp
    src/resource_roles.cpp
    src/constant_buffer_read.cpp
    src/resource_ref.cpp
    src/frame_tap.cpp
    src/depth_replay.cpp
    src/d3d11_state.cpp
    src/scene_reinsert.cpp
    src/present_blit.cpp
    src/overlay_renderer.cpp
    src/overlay_input.cpp
    src/ui_identify.cpp
    src/d3d11_observer.cpp)
# The overlay's C ABI lives with the Rust crate that implements it, because that crate is the side
# the contract exists to constrain. The renderer and the input capture only consume it.
target_include_directories(rsf_graphics PUBLIC "${CMAKE_SOURCE_DIR}/ui/overlay/include")
target_include_directories(rsf_graphics PUBLIC include)
target_link_libraries(rsf_graphics PRIVATE rsf_build_options)
target_link_libraries(rsf_graphics PUBLIC d3d11 dxgi)
add_library(ReScaleFrame::Graphics ALIAS rsf_graphics)
