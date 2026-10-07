# Cross-API sharing and physical presentation ownership, separate from D3D11-only graphics.
# Public dependencies expose the selected vendor/provider contracts to runtime consumers.
add_library(rsf_presentation STATIC
    src/shared_surface.cpp
    src/d3d11_present_bridge.cpp)
target_include_directories(rsf_presentation PUBLIC include)
# Build host declarations for the in-process Streamline owner; no second plugin-owned host.
target_compile_definitions(rsf_presentation PRIVATE RSF_STREAMLINE_HOST_BUILD)
target_link_libraries(rsf_presentation PRIVATE rsf_build_options)
target_link_libraries(rsf_presentation PUBLIC d3d11 d3d12 dxgi ReScaleFrame::DLSS ReScaleFrame::FSR ReScaleFrame::XeSS ReScaleFrame::Graphics minhook)
add_library(ReScaleFrame::Presentation ALIAS rsf_presentation)
