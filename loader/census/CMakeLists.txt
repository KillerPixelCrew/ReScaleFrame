# Vendor call census: a diagnostic probe that records calls into vendor upscaler, frame
# generation, latency and presentation entry points. MSVC only: the decoders read game memory
# under structured exception handling.
if(NOT MSVC)
    message(STATUS "rsf_vendor_census needs MSVC structured exception handling; skipped")
    return()
endif()

add_library(rsf_vendor_census STATIC
    src/census_platform.cpp
    src/decode_d3d_platform.cpp
    src/guarded_platform.cpp
    src/hook_platform.cpp
    src/d3d_guarded_platform.cpp)
target_include_directories(rsf_vendor_census PUBLIC include PRIVATE src)
target_compile_features(rsf_vendor_census PUBLIC cxx_std_20)
target_link_libraries(rsf_vendor_census PRIVATE rsf_loader_rust minhook rsf_build_options)
add_library(ReScaleFrame::VendorCensus ALIAS rsf_vendor_census)
