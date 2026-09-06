add_library(rsf_loader_diagnostics STATIC
    src/entropy.c
    src/module_dump.c)
target_include_directories(rsf_loader_diagnostics PUBLIC include)
target_link_libraries(rsf_loader_diagnostics PRIVATE rsf_build_options)
# The dump writer uses the historical CRT names deliberately; the _s variants are not portable to
# the mingw cross build that this code is developed against.
target_compile_definitions(rsf_loader_diagnostics PRIVATE _CRT_SECURE_NO_WARNINGS)
add_library(ReScaleFrame::LoaderDiagnostics ALIAS rsf_loader_diagnostics)
