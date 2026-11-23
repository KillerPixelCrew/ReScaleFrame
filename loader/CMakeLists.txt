rsf_add_rust_static(rsf_loader_rust rsf_rust_loader rsf-loader rsf_loader)
# rsf-minhook-sys leaves MinHook to the CMake link (third_party/minhook/build.rs).
target_link_libraries(rsf_loader_rust INTERFACE minhook)
add_subdirectory(diagnostics)
add_subdirectory(proxy)
add_subdirectory(census)

