# rsf_get_d3d11_fg_api client. Like d3d11_present_api.cmake: import the Runtime DLL only, never
# OrchestratorCore and its public RSF_RUNTIME_BUILD define.
rsf_add_runtime_client(rsf_d3d11_fg_api_test "${CMAKE_CURRENT_LIST_DIR}/generation/dll_main.c")
target_link_libraries(rsf_d3d11_fg_api_test PRIVATE rsf_generation_fixtures_rust)
rsf_add_c11_header_check(rsf_d3d11_fg_api_c11 "${CMAKE_CURRENT_LIST_DIR}/d3d11_fg_api_c11.c")
add_test(NAME d3d11_fg_api_contract COMMAND rsf_d3d11_fg_api_test)
set_tests_properties(d3d11_fg_api_contract PROPERTIES TIMEOUT 30)
