set(RSF_FFX_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/fidelityfx/Kits/FidelityFX/api/include" CACHE PATH
    "Directory holding FidelityFX's ffx_api.h")

add_library(rsf_fsr STATIC src/fsr_backend.cpp src/fsr_generation.cpp src/fsr4_compat.cpp)
target_include_directories(rsf_fsr PUBLIC include)
target_link_libraries(rsf_fsr PRIVATE rsf_build_options minhook bcrypt)
target_link_libraries(rsf_fsr PUBLIC ReScaleFrame::Contract d3d12 dxgi)

# Fetched into an untracked vendor directory, exactly as Streamline is, so a checkout without it
# still builds and tests. What is lost is the implementation and not the contract: the provider
# still exists and every entry point reports NOT_COMPILED, which a caller can tell apart from
# FidelityFX being present and refusing. That distinction is the reason the probe returns a reason
# string rather than a boolean.
if(EXISTS "${RSF_FFX_INCLUDE}/ffx_api.h")
    target_compile_definitions(rsf_fsr PUBLIC RSF_HAVE_FFX=1)
    target_include_directories(rsf_fsr SYSTEM PRIVATE "${RSF_FFX_INCLUDE}/../../upscalers/include")
    # AMD's headers are not compiled with our warning settings and are not ours to change.
    if(MSVC)
        target_compile_options(rsf_fsr PRIVATE /external:I "${RSF_FFX_INCLUDE}" /external:W0)
    else()
        target_include_directories(rsf_fsr SYSTEM PRIVATE "${RSF_FFX_INCLUDE}")
    endif()
else()
    message(STATUS "ffx_api.h not found in ${RSF_FFX_INCLUDE}; FSR support disabled")
endif()

add_library(ReScaleFrame::FSR ALIAS rsf_fsr)

if(EXISTS "${RSF_FFX_INCLUDE}/../../framegeneration/include/ffx_framegeneration.h")
    target_compile_definitions(rsf_fsr PRIVATE RSF_HAVE_FFX_FG=1)
    target_include_directories(rsf_fsr SYSTEM PRIVATE "${RSF_FFX_INCLUDE}/../../framegeneration/include")
endif()
