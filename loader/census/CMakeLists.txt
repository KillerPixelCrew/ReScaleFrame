# Vendor call census: a diagnostic probe that records calls into vendor upscaler, frame
# generation, latency and presentation entry points. MSVC only: the native leaves read game memory
# under structured exception handling (RSF_FAULT_CATCH, rescaleframe/foreign_guard.h). The census
# itself is Rust; the C11 leaves hold SEH, MinHook and vendor SDK calls that Rust cannot express.
if(NOT MSVC)
    message(STATUS "rsf_vendor_census needs MSVC structured exception handling; skipped")
    return()
endif()

add_library(rsf_vendor_census STATIC
    src/census_platform.c
    src/guarded_platform.c
    src/hook_platform.c
    src/d3d_guarded_platform.c)
target_include_directories(rsf_vendor_census PUBLIC include PRIVATE src)
target_link_libraries(rsf_vendor_census PRIVATE rsf_loader_rust ReScaleFrame::GameSDK minhook rsf_build_options)
add_library(ReScaleFrame::VendorCensus ALIAS rsf_vendor_census)
