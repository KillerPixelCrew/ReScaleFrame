# The FidelityFX providers are Rust in the Runtime archive. FidelityFX is fetched into an untracked
# vendor directory, exactly as Streamline is, so a checkout without it still builds and tests. What
# is lost is the implementation and not the contract: the provider still exists and every entry
# point reports NOT_COMPILED, which a caller can tell apart from FidelityFX being present and
# refusing. cmake/vendor.cmake sets RSF_HAVE_FFX and RSF_HAVE_FFX_FG, and
# runtime/orchestrator/CMakeLists.txt turns them into the archive's Cargo features.
add_library(rsf_fsr INTERFACE)
target_link_libraries(rsf_fsr INTERFACE ReScaleFrame::Contract rsf_orchestrator_rust d3d12 dxgi)
add_library(ReScaleFrame::FSR ALIAS rsf_fsr)
if(NOT RSF_HAVE_FFX)
    message(STATUS "ffx_api.h not found in ${RSF_FFX_INCLUDE}; FSR support disabled")
endif()