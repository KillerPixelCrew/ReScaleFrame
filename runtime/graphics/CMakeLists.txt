add_library(rsf_graphics STATIC
    src/texture_dump.c)
# The overlay's C ABI lives with the Rust crate that implements it, because that crate is the side
# the contract exists to constrain. The renderer and the input capture only consume it.
target_include_directories(rsf_graphics PUBLIC "${CMAKE_SOURCE_DIR}/ui/overlay/include")
target_include_directories(rsf_graphics PUBLIC include)
target_link_libraries(rsf_graphics PRIVATE rsf_build_options)
target_link_libraries(rsf_graphics PUBLIC d3d11 d3d12 dxgi)
target_link_libraries(rsf_graphics PRIVATE windowscodecs ole32 psapi)
target_link_libraries(rsf_graphics PRIVATE minhook)
target_link_libraries(rsf_graphics PUBLIC ReScaleFrame::RuntimeCommon)
add_library(ReScaleFrame::Graphics ALIAS rsf_graphics)
# The Rust D3D11 leaves (rsf-graphics). The Runtime embeds them through its own archive, so only
# targets that do not link the Runtime archive (game adapters and their fixtures) link this one.
rsf_add_rust_static(rsf_graphics_rust rsf_rust_graphics rsf-graphics rsf_graphics)
target_link_libraries(rsf_graphics_rust INTERFACE rsf_graphics minhook)
add_library(ReScaleFrame::GraphicsLeaves ALIAS rsf_graphics_rust)
