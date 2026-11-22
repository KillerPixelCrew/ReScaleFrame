# Shared C provider and frame-generation contracts.
add_subdirectory("${CMAKE_SOURCE_DIR}/runtime/common" "${CMAKE_BINARY_DIR}/runtime/common")
add_library(rsf_contract INTERFACE)
target_include_directories(rsf_contract INTERFACE include)
target_link_libraries(rsf_contract INTERFACE ReScaleFrame::GameSDK)
add_library(ReScaleFrame::Contract ALIAS rsf_contract)
