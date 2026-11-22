# Helpers shared by runtime libraries and independently linked adapters and carriers.
add_library(rsf_runtime_common INTERFACE)
target_include_directories(rsf_runtime_common INTERFACE include)
rsf_add_rust_static(rsf_common_rust rsf_rust_common rsf-runtime-common rsf_runtime_common)
target_link_libraries(rsf_runtime_common INTERFACE rsf_common_rust bcrypt)
add_library(ReScaleFrame::RuntimeCommon ALIAS rsf_runtime_common)
