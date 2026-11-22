rsf_add_rust_static(rsf_shader_compiler rsf_shader_compiler_build rsf-shader-compiler rsf_shader_compiler)
add_library(rsf_graphics STATIC
    src/texture_dump.c
    src/observer_exception_guards.cpp)
# The overlay's C ABI lives with the Rust crate that implements it, because that crate is the side
# the contract exists to constrain. The renderer and the input capture only consume it.
target_include_directories(rsf_graphics PUBLIC "${CMAKE_SOURCE_DIR}/ui/overlay/include")
target_include_directories(rsf_graphics PUBLIC include)
target_link_libraries(rsf_graphics PRIVATE rsf_build_options)
target_link_libraries(rsf_graphics PUBLIC d3d11 d3d12 dxgi rsf_shader_compiler)
target_link_libraries(rsf_graphics PRIVATE windowscodecs ole32 psapi)
target_link_libraries(rsf_graphics PRIVATE minhook)
# Both observers dispatch in Rust and use a native frame for foreign exceptions.
target_link_libraries(rsf_graphics PRIVATE rsf_orchestrator_rust)
target_link_libraries(rsf_graphics PUBLIC ReScaleFrame::RuntimeCommon)
add_library(ReScaleFrame::Graphics ALIAS rsf_graphics)
