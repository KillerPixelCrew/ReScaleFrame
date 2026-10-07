# Public C ABI headers plus pure backend negotiation. GameSDK supplies the shared frame records;
# negotiation has no graphics or vendor SDK dependency.
add_library(rsf_contract STATIC src/negotiate.cpp)
target_include_directories(rsf_contract PUBLIC include)
target_link_libraries(rsf_contract PUBLIC ReScaleFrame::GameSDK)
target_link_libraries(rsf_contract PRIVATE rsf_build_options)
add_library(ReScaleFrame::Contract ALIAS rsf_contract)
