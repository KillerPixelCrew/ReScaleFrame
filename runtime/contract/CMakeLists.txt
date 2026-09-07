# The vendor-neutral contract.
#
# The headers declare what a backend is and implement nothing, so a backend compiles against them
# without linking anything. Negotiation is the one piece with behaviour, and it is here rather than
# in the orchestrator because it is a pure function over what the vendors said: no devices, no
# loading, no state. That is what lets the awkward vendor combinations be tested on a machine that
# has none of the hardware, which matters because those are exactly the combinations nobody can try.
add_library(rsf_contract STATIC src/negotiate.cpp)
target_include_directories(rsf_contract PUBLIC include)
target_link_libraries(rsf_contract PUBLIC ReScaleFrame::GameSDK)
target_link_libraries(rsf_contract PRIVATE rsf_build_options)
add_library(ReScaleFrame::Contract ALIAS rsf_contract)
