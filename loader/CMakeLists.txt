rsf_add_rust_static(rsf_loader_rust rsf_rust_loader rsf-loader rsf_loader)
add_subdirectory(diagnostics)
add_subdirectory(proxy)
add_subdirectory(census)

add_library(rsf_bootstrap SHARED src/bootstrap.c)
target_link_libraries(rsf_bootstrap PRIVATE rsf_loader_rust rsf_build_options)
set_target_properties(rsf_bootstrap PROPERTIES OUTPUT_NAME "ReScaleFrame.Bootstrap")
