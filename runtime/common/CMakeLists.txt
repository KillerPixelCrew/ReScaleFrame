# Header-only helpers shared by the runtime libraries that cannot depend on one another: graphics,
# presentation, orchestrator and the vendor backend libraries. Internal, not part of any ABI.
add_library(rsf_runtime_common INTERFACE)
target_include_directories(rsf_runtime_common INTERFACE include)
# sha256_file.h calls BCrypt.
target_link_libraries(rsf_runtime_common INTERFACE bcrypt)
add_library(ReScaleFrame::RuntimeCommon ALIAS rsf_runtime_common)
