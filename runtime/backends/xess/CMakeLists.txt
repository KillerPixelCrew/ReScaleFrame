# XeSS/XeFG/XeLL exports are loaded at runtime; this path supplies only compile-time types.
set(RSF_XESS_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/xess/inc" CACHE PATH
    "Directory holding XeSS's xess/xess.h")

add_library(rsf_xess STATIC src/xess_backend.cpp src/xess_generation.cpp)
target_include_directories(rsf_xess PUBLIC include)
target_link_libraries(rsf_xess PRIVATE rsf_build_options)
target_link_libraries(rsf_xess PUBLIC ReScaleFrame::Contract d3d11 d3d12 dxgi)

# Keep provider symbols with NOT_COMPILED stubs when the optional SR SDK headers are absent.
# The implemented SR adapter currently accepts DX12 only.
if(EXISTS "${RSF_XESS_INCLUDE}/xess/xess.h")
    target_compile_definitions(rsf_xess PUBLIC RSF_HAVE_XESS=1)
    # Suppress external SDK warnings while retaining first-party warning policy.
    if(MSVC)
        target_compile_options(rsf_xess PRIVATE /external:I "${RSF_XESS_INCLUDE}" /external:W0)
    else()
        target_include_directories(rsf_xess SYSTEM PRIVATE "${RSF_XESS_INCLUDE}")
    endif()
else()
    message(STATUS "xess/xess.h not found in ${RSF_XESS_INCLUDE}; XeSS support disabled")
endif()

add_library(ReScaleFrame::XeSS ALIAS rsf_xess)

# Live generation requires both the XeFG proxy-chain API and its XeLL pacing API.
if(EXISTS "${RSF_XESS_INCLUDE}/xess_fg/xefg_swapchain_d3d12.h" AND
   EXISTS "${RSF_XESS_INCLUDE}/xell/xell_d3d12.h")
    target_compile_definitions(rsf_xess PRIVATE RSF_HAVE_XESS_FG=1)
endif()
