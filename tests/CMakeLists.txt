add_library(rsf_sdk_c_header OBJECT sdk_c_header.c)
target_link_libraries(rsf_sdk_c_header PRIVATE ReScaleFrame::GameSDK rsf_build_options)

add_executable(rsf_plugin_contract plugin_contract.cpp)
target_link_libraries(rsf_plugin_contract PRIVATE ReScaleFrame::GameSDK rsf_build_options)
add_dependencies(rsf_plugin_contract rsf_game_ac7)
target_compile_definitions(rsf_plugin_contract PRIVATE _CRT_SECURE_NO_WARNINGS)
if(NOT MSVC)
    # This test uses wmain, which mingw only selects as the entry point with -municode.
    target_compile_options(rsf_plugin_contract PRIVATE -municode)
    target_link_options(rsf_plugin_contract PRIVATE -municode)
endif()
# Load the plugin by name from its own directory. That is how it is loaded in practice, and it
# keeps the test runnable under a cross-build emulator, which cannot take a host path.
add_test(NAME game_plugin_contract COMMAND rsf_plugin_contract "$<TARGET_FILE_NAME:rsf_game_ac7>")
set_tests_properties(game_plugin_contract PROPERTIES
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_game_ac7>")
add_executable(rsf_module_dump_entropy module_dump_entropy.c)
target_link_libraries(rsf_module_dump_entropy PRIVATE ReScaleFrame::LoaderDiagnostics rsf_build_options)
add_test(NAME module_dump_entropy COMMAND rsf_module_dump_entropy)

add_executable(rsf_module_dump_self module_dump_self.c)
target_link_libraries(rsf_module_dump_self PRIVATE ReScaleFrame::LoaderDiagnostics rsf_build_options)
target_compile_definitions(rsf_module_dump_self PRIVATE _CRT_SECURE_NO_WARNINGS)
add_test(NAME module_dump_self COMMAND rsf_module_dump_self "${CMAKE_CURRENT_BINARY_DIR}")

add_test(NAME launcher_version COMMAND rsf_launcher --version)
set_tests_properties(launcher_version PROPERTIES PASS_REGULAR_EXPRESSION "ReScaleFrame ${PROJECT_VERSION}")
