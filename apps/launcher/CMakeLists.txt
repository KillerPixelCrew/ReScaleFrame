add_executable(rsf_launcher src/main.c)
target_link_libraries(rsf_launcher PRIVATE rsf_loader_rust rsf_build_options)
set_target_properties(rsf_launcher PROPERTIES OUTPUT_NAME "ReScaleFrame")
