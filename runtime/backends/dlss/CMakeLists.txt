set(RSF_STREAMLINE_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/streamline/include" CACHE PATH
    "Directory holding Streamline's sl.h")

add_library(rsf_dlss STATIC src/dlss_streamline.cpp src/dlss_generation.cpp src/dlss_native12.cpp src/reflex_audit.cpp)
target_compile_definitions(rsf_dlss PRIVATE RSF_STREAMLINE_HOST_BUILD)
target_include_directories(rsf_dlss PUBLIC include)
target_link_libraries(rsf_dlss PRIVATE rsf_build_options)
target_link_libraries(rsf_dlss PUBLIC ReScaleFrame::Contract d3d11 d3d12 dxgi)
target_link_libraries(rsf_dlss PRIVATE ReScaleFrame::RuntimeCommon)

# Streamline is fetched into an untracked vendor directory, so a checkout without it still builds
# and tests. What is lost is the implementation, not the contract: the entry points still exist and
# report RSF_DLSS_ERROR_NOT_COMPILED, which is a different thing from DLSS failing.
if(EXISTS "${RSF_STREAMLINE_INCLUDE}/sl.h")
    target_include_directories(rsf_dlss PRIVATE "${RSF_STREAMLINE_INCLUDE}")
    target_compile_definitions(rsf_dlss PUBLIC RSF_HAVE_STREAMLINE=1)
    # NVIDIA's headers are not compiled with our warning settings and are not ours to change.
    if(MSVC)
        target_compile_options(rsf_dlss PRIVATE /external:I "${RSF_STREAMLINE_INCLUDE}" /external:W0)
    else()
        target_include_directories(rsf_dlss SYSTEM PRIVATE "${RSF_STREAMLINE_INCLUDE}")
    endif()
else()
    message(STATUS "sl.h not found in ${RSF_STREAMLINE_INCLUDE}; DLSS support disabled")
endif()

add_library(ReScaleFrame::DLSS ALIAS rsf_dlss)

# Optional read-only driver diagnostics. Streamline still owns Reflex mode, sleep and markers.
find_path(RSF_NVAPI_INCLUDE nvapi.h HINTS
    "${CMAKE_SOURCE_DIR}/vendor/nvapi"
    "${CMAKE_SOURCE_DIR}/.local/streamline/sdk/external/nvapi")
if(RSF_NVAPI_INCLUDE)
    target_link_libraries(rsf_dlss PRIVATE minhook)
    target_include_directories(rsf_dlss SYSTEM PRIVATE "${RSF_NVAPI_INCLUDE}")
    target_compile_definitions(rsf_dlss PRIVATE RSF_HAVE_NVAPI_DIAGNOSTICS=1)
endif()
