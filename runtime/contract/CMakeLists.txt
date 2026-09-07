# The vendor-neutral contract. Headers only: it declares what a backend is and implements nothing,
# so a backend can be compiled against it without linking anything.
add_library(rsf_contract INTERFACE)
target_include_directories(rsf_contract INTERFACE include)
target_link_libraries(rsf_contract INTERFACE ReScaleFrame::GameSDK)
add_library(ReScaleFrame::Contract ALIAS rsf_contract)
