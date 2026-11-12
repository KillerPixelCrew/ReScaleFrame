# Include from the root after add_subdirectory(loader/proxy), or use thin_loader_inject.cmake.
include_guard(GLOBAL)
add_executable(rsf_thin_loader_test "${CMAKE_CURRENT_LIST_DIR}/thin_loader.cpp")
target_link_libraries(rsf_thin_loader_test PRIVATE rsf_build_options dxguid user32)
if(TARGET rsf_proxy_thin_dinput8)
    add_dependencies(rsf_thin_loader_test rsf_proxy_thin_dinput8)
endif()
add_test(NAME thin_loader_pe COMMAND rsf_thin_loader_test pe
    "${CMAKE_BINARY_DIR}/thin/bin/$<CONFIG>/dinput8.dll")
set_tests_properties(thin_loader_pe PROPERTIES TIMEOUT 30)
add_test(NAME thin_loader_forward COMMAND rsf_thin_loader_test forward
    "${CMAKE_BINARY_DIR}/thin/bin/$<CONFIG>/dinput8.dll")
set_tests_properties(thin_loader_forward PROPERTIES TIMEOUT 30)
add_library(rsf_thin_loader_fixture_runtime SHARED "${CMAKE_CURRENT_LIST_DIR}/thin_loader_fixture.cpp")
target_compile_definitions(rsf_thin_loader_fixture_runtime PRIVATE THIN_FIXTURE_RUNTIME)
add_library(rsf_thin_loader_fixture_adapter SHARED "${CMAKE_CURRENT_LIST_DIR}/thin_loader_fixture.cpp")
foreach(fixture rsf_thin_loader_fixture_runtime rsf_thin_loader_fixture_adapter)
    target_link_libraries(${fixture} PRIVATE rsf_build_options)
    set_target_properties(${fixture} PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/thin/fixtures/$<CONFIG>")
    add_dependencies(rsf_thin_loader_test ${fixture})
endforeach()
foreach(scenario startup malformed duplicate escape junction
    empty missing_section missing_auto missing_entry empty_runtime empty_entry invalid_symbol unknown_key
    duplicate_section syntax nul bad_utf8 oversized absolute rooted drive_relative unc ads trailing_dot
    empty_component parent_component adapter_escape same_module self_module disabled default utf8_bom
    utf16 shared_config missing_export hardlink_alias refuse missing_config missing_runtime missing_adapter
    bad_runtime_image bad_adapter_image concurrent scoped_modes)
    add_test(NAME thin_loader_${scenario} COMMAND rsf_thin_loader_test ${scenario}
        "${CMAKE_BINARY_DIR}/thin/bin/$<CONFIG>/dinput8.dll"
        "$<TARGET_FILE:rsf_thin_loader_fixture_runtime>" "$<TARGET_FILE:rsf_thin_loader_fixture_adapter>")
    set_tests_properties(thin_loader_${scenario} PROPERTIES TIMEOUT 30)
endforeach()
