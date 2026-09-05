add_library(rsf_sdk_c_header OBJECT sdk_c_header.c)
target_link_libraries(rsf_sdk_c_header PRIVATE ReScaleFrame::GameSDK rsf_build_options)

add_executable(rsf_plugin_contract plugin_contract.cpp)
target_link_libraries(rsf_plugin_contract PRIVATE ReScaleFrame::GameSDK rsf_build_options)
add_dependencies(rsf_plugin_contract rsf_game_ac7)
add_test(NAME game_plugin_contract COMMAND rsf_plugin_contract "$<TARGET_FILE:rsf_game_ac7>")
add_test(NAME launcher_version COMMAND rsf_launcher --version)
set_tests_properties(launcher_version PROPERTIES PASS_REGULAR_EXPRESSION "ReScaleFrame ${PROJECT_VERSION}")
