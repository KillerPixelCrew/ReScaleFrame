add_library(rsf_bootstrap SHARED src/bootstrap.cpp)
target_link_libraries(rsf_bootstrap PRIVATE rsf_build_options)
set_target_properties(rsf_bootstrap PROPERTIES OUTPUT_NAME "ReScaleFrame.Bootstrap")
