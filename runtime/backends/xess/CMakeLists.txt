set(RSF_XESS_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/xess/inc" CACHE PATH
    "Directory holding XeSS's xess/xess.h")

add_library(rsf_xess STATIC src/xess_backend.cpp)
target_include_directories(rsf_xess PUBLIC include)
target_link_libraries(rsf_xess PRIVATE rsf_build_options)
target_link_libraries(rsf_xess PUBLIC ReScaleFrame::Contract d3d11 d3d12 dxgi)

# As with the other two: fetched into an untracked vendor directory, so a checkout without it still
# builds and tests, and the provider answers NOT_COMPILED rather than failing to link.
#
# XeSS links d3d11 as well as d3d12, which the others do not, because it is the only vendor that
# reconstructs on D3D11 at all. That path is Intel hardware only, which the probe reports rather
# than discovering at evaluate time.
if(EXISTS "${RSF_XESS_INCLUDE}/xess/xess.h")
    target_compile_definitions(rsf_xess PUBLIC RSF_HAVE_XESS=1)
    # Intel's headers are not compiled with our warning settings and are not ours to change.
    if(MSVC)
        target_compile_options(rsf_xess PRIVATE /external:I "${RSF_XESS_INCLUDE}" /external:W0)
    else()
        target_include_directories(rsf_xess SYSTEM PRIVATE "${RSF_XESS_INCLUDE}")
    endif()
else()
    message(STATUS "xess/xess.h not found in ${RSF_XESS_INCLUDE}; XeSS support disabled")
endif()

add_library(ReScaleFrame::XeSS ALIAS rsf_xess)
