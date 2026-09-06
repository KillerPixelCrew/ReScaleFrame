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

add_executable(rsf_texture_dump texture_dump.cpp)
target_link_libraries(rsf_texture_dump PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME texture_dump COMMAND rsf_texture_dump "${CMAKE_CURRENT_BINARY_DIR}")

add_executable(rsf_ac7_view_test ac7_view.cpp)
target_link_libraries(rsf_ac7_view_test PRIVATE ReScaleFrame::AC7View rsf_build_options)
add_test(NAME ac7_view COMMAND rsf_ac7_view_test)

add_executable(rsf_motion_decode motion_decode.cpp)
target_link_libraries(rsf_motion_decode PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME motion_decode COMMAND rsf_motion_decode)
set_tests_properties(motion_decode PROPERTIES TIMEOUT 120)

add_executable(rsf_d3d11_observer d3d11_observer.cpp)
target_link_libraries(rsf_d3d11_observer PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME d3d11_observer COMMAND rsf_d3d11_observer "${CMAKE_CURRENT_BINARY_DIR}")
# These touch a graphics runtime and patch vtables, so a deadlock is a plausible failure. Fail in
# a couple of minutes rather than hanging the suite.
set_tests_properties(d3d11_observer texture_dump PROPERTIES TIMEOUT 120)

add_executable(rsf_dlss_backend dlss_backend.cpp)
target_link_libraries(rsf_dlss_backend PRIVATE ReScaleFrame::DLSS rsf_build_options)
add_test(NAME dlss_backend COMMAND rsf_dlss_backend)
# Loading a vendor runtime that talks to a driver is another place a hang is plausible.
set_tests_properties(dlss_backend PROPERTIES TIMEOUT 120)

add_test(NAME launcher_version COMMAND rsf_launcher --version)
set_tests_properties(launcher_version PROPERTIES PASS_REGULAR_EXPRESSION "ReScaleFrame ${PROJECT_VERSION}")
