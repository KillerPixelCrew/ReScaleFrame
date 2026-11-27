# Presentation: everything between the game's finished frame and the screen.
#
# Separate from Graphics because it links d3d12, which nothing in the D3D11 path should have to.
add_library(rsf_presentation STATIC
    src/d3d11_present_bridge.c)
target_include_directories(rsf_presentation PUBLIC include)
target_compile_definitions(rsf_presentation PRIVATE RSF_STREAMLINE_HOST_BUILD)
target_link_libraries(rsf_presentation PRIVATE rsf_build_options opengl32)
target_link_libraries(rsf_presentation PUBLIC d3d11 d3d12 dxgi ReScaleFrame::DLSS ReScaleFrame::FSR ReScaleFrame::XeSS ReScaleFrame::Graphics minhook)
# The SEH-guarded native calls and the factory detours of the Rust presentation owner, which
# calls into them and back.
target_link_libraries(rsf_presentation PUBLIC rsf_orchestrator_rust)
add_library(ReScaleFrame::Presentation ALIAS rsf_presentation)
