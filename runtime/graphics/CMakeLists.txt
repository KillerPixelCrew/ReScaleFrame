rsf_add_rust_static(rsf_shader_compiler rsf_shader_compiler_build rsf-shader-compiler rsf_shader_compiler)
add_library(rsf_graphics STATIC
    src/texture_dump.c
    src/overlay_renderer.cpp
    src/d3d11_observer.cpp)
target_sources(rsf_graphics PRIVATE src/d3d12_observer.cpp)
# The overlay's C ABI lives with the Rust crate that implements it, because that crate is the side
# the contract exists to constrain. The renderer and the input capture only consume it.
target_include_directories(rsf_graphics PUBLIC "${CMAKE_SOURCE_DIR}/ui/overlay/include")
target_include_directories(rsf_graphics PUBLIC include)
target_link_libraries(rsf_graphics PRIVATE rsf_build_options)
target_link_libraries(rsf_graphics PUBLIC d3d11 d3d12 dxgi rsf_shader_compiler)
target_link_libraries(rsf_graphics PRIVATE windowscodecs ole32 psapi)
target_link_libraries(rsf_graphics PRIVATE minhook)
# The D3D12 observer supplies native leaves to the Rust owner.
target_link_libraries(rsf_graphics PRIVATE rsf_orchestrator_rust)
# Public: shader_compile.h includes log.h, and presentation and the orchestrator reach it through here.
target_link_libraries(rsf_graphics PUBLIC ReScaleFrame::RuntimeCommon)
# MinHook's length disassembler, for building a trampoline over a function's bytes as the file has
# them rather than as memory has them. It is compiled into the library but not exported by its
# public header.
target_include_directories(rsf_graphics PRIVATE "${minhook_SOURCE_DIR}/src/hde")
add_library(ReScaleFrame::Graphics ALIAS rsf_graphics)
