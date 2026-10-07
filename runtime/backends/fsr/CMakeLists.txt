# FFX symbols are resolved from deployed DLLs at runtime; SDK headers are optional at build time.
set(RSF_FFX_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/fidelityfx/Kits/FidelityFX/api/include" CACHE PATH
    "Directory holding FidelityFX's ffx_api.h")

add_library(rsf_fsr STATIC src/fsr_backend.cpp src/fsr_generation.cpp src/fsr4_compat.cpp)
target_include_directories(rsf_fsr PUBLIC include)
target_link_libraries(rsf_fsr PRIVATE rsf_build_options minhook bcrypt)
target_link_libraries(rsf_fsr PUBLIC ReScaleFrame::Contract d3d12 dxgi)

# Without SR headers, keep the provider symbols and compile NOT_COMPILED stubs.
if(EXISTS "${RSF_FFX_INCLUDE}/ffx_api.h")
    target_compile_definitions(rsf_fsr PUBLIC RSF_HAVE_FFX=1)
    target_include_directories(rsf_fsr SYSTEM PRIVATE "${RSF_FFX_INCLUDE}/../../upscalers/include")
    # Suppress external SDK warnings while retaining first-party warning policy.
    if(MSVC)
        target_compile_options(rsf_fsr PRIVATE /external:I "${RSF_FFX_INCLUDE}" /external:W0)
    else()
        target_include_directories(rsf_fsr SYSTEM PRIVATE "${RSF_FFX_INCLUDE}")
    endif()
else()
    message(STATUS "ffx_api.h not found in ${RSF_FFX_INCLUDE}; FSR support disabled")
endif()

add_library(ReScaleFrame::FSR ALIAS rsf_fsr)

# The newer generation table has its own header gate, independent of the legacy SR/FG table.
if(EXISTS "${RSF_FFX_INCLUDE}/../../framegeneration/include/ffx_framegeneration.h")
    target_compile_definitions(rsf_fsr PRIVATE RSF_HAVE_FFX_FG=1)
    target_include_directories(rsf_fsr SYSTEM PRIVATE "${RSF_FFX_INCLUDE}/../../framegeneration/include")
endif()
