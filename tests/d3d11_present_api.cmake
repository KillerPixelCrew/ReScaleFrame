# Include at the end of the root build (or from tests/CMakeLists.txt).
# Keep these clients free of OrchestratorCore and its public RSF_RUNTIME_BUILD define.
add_executable(rsf_d3d11_present_api_test "${CMAKE_CURRENT_LIST_DIR}/d3d11_present_api.cpp")
add_executable(rsf_d3d11_present_api_c11 "${CMAKE_CURRENT_LIST_DIR}/d3d11_present_api_c11.c")
set_target_properties(rsf_d3d11_present_api_c11 PROPERTIES
    C_STANDARD 11 C_STANDARD_REQUIRED ON C_EXTENSIONS OFF)
foreach(client rsf_d3d11_present_api_test rsf_d3d11_present_api_c11)
    target_include_directories(${client} PRIVATE
        "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
    # Import only: do not inherit OrchestratorCore's public RSF_RUNTIME_BUILD define.
    target_link_libraries(${client} PRIVATE "$<TARGET_LINKER_FILE:rsf_orchestrator>" rsf_build_options)
    add_dependencies(${client} rsf_orchestrator)
endforeach()
add_test(NAME d3d11_present_api_contract COMMAND rsf_d3d11_present_api_test)
add_test(NAME d3d11_present_api_c11_contract COMMAND rsf_d3d11_present_api_c11)
set_tests_properties(d3d11_present_api_contract d3d11_present_api_c11_contract PROPERTIES TIMEOUT 30)
