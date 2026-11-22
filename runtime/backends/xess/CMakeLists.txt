# The XeSS providers are Rust in the Runtime archive. As with the other two, XeSS is fetched into an
# untracked vendor directory, so a checkout without it still builds and tests, and the provider
# answers NOT_COMPILED rather than failing to link. cmake/vendor.cmake sets RSF_HAVE_XESS and
# RSF_HAVE_XESS_FG, and runtime/orchestrator/CMakeLists.txt turns them into Cargo features.
#
# XeSS links d3d11 as well as d3d12, which the others do not, because it is the only vendor that
# reconstructs on D3D11 at all. That path is Intel hardware only, which the probe reports rather
# than discovering at evaluate time.
add_library(rsf_xess INTERFACE)
target_link_libraries(rsf_xess INTERFACE ReScaleFrame::Contract rsf_orchestrator_rust d3d11 d3d12 dxgi)
add_library(ReScaleFrame::XeSS ALIAS rsf_xess)
if(NOT RSF_HAVE_XESS)
    message(STATUS "xess/xess.h not found in ${RSF_XESS_INCLUDE}; XeSS support disabled")
endif()