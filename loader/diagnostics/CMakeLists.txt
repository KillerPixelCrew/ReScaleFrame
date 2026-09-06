set(RSF_RENDERDOC_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/renderdoc" CACHE PATH
    "Directory holding RenderDoc's renderdoc_app.h")

add_library(rsf_loader_diagnostics STATIC
    src/entropy.c
    src/module_dump.c)
target_include_directories(rsf_loader_diagnostics PUBLIC include)

# The RenderDoc header is fetched into an untracked vendor directory, so capture support is
# optional and its absence only costs the capture trigger.
if(EXISTS "${RSF_RENDERDOC_INCLUDE}/renderdoc_app.h")
    target_sources(rsf_loader_diagnostics PRIVATE src/frame_capture.c)
    target_include_directories(rsf_loader_diagnostics PRIVATE "${RSF_RENDERDOC_INCLUDE}")
    target_compile_definitions(rsf_loader_diagnostics PUBLIC RSF_HAVE_FRAME_CAPTURE=1)
else()
    message(STATUS "renderdoc_app.h not found in ${RSF_RENDERDOC_INCLUDE}; capture disabled")
endif()
target_link_libraries(rsf_loader_diagnostics PRIVATE rsf_build_options)
# The dump writer uses the historical CRT names deliberately; the _s variants are not portable to
# the mingw cross build that this code is developed against.
target_compile_definitions(rsf_loader_diagnostics PRIVATE _CRT_SECURE_NO_WARNINGS)
add_library(ReScaleFrame::LoaderDiagnostics ALIAS rsf_loader_diagnostics)
