# Clients of rsf_get_overlay_api that import ReScaleFrame.Runtime.dll the way a DLL-loaded
# adapter does. Keep them free of OrchestratorCore and its public RSF_RUNTIME_BUILD define.
add_executable(rsf_overlay_dll_api_test "${CMAKE_CURRENT_LIST_DIR}/overlay_dll_api.cpp")
add_executable(rsf_overlay_dll_api_c11 "${CMAKE_CURRENT_LIST_DIR}/overlay_dll_api_c11.c")
add_executable(rsf_overlay_dll_api_device "${CMAKE_CURRENT_LIST_DIR}/overlay_dll_api_device.cpp")
set_target_properties(rsf_overlay_dll_api_c11 PROPERTIES
    C_STANDARD 11 C_STANDARD_REQUIRED ON C_EXTENSIONS OFF)
foreach(client rsf_overlay_dll_api_test rsf_overlay_dll_api_c11 rsf_overlay_dll_api_device)
    target_include_directories(${client} PRIVATE
        "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
    target_link_libraries(${client} PRIVATE "$<TARGET_LINKER_FILE:rsf_orchestrator>" rsf_build_options)
    add_dependencies(${client} rsf_orchestrator)
endforeach()
target_link_libraries(rsf_overlay_dll_api_device PRIVATE d3d11 dxgi)
add_test(NAME overlay_dll_api_contract COMMAND rsf_overlay_dll_api_test)
add_test(NAME overlay_dll_api_c11_contract COMMAND rsf_overlay_dll_api_c11)
set_tests_properties(overlay_dll_api_contract overlay_dll_api_c11_contract PROPERTIES TIMEOUT 30)

# The device fixture loads the real Rust panel. Build it first with
#   cargo build --release --locked -p rescaleframe-overlay
# The test is registered only when that DLL exists at configure time; it fails, not skips,
# when the DLL disappears later, and skips (77) only without any D3D11 device.
set(RSF_OVERLAY_PANEL_DLL "${CMAKE_SOURCE_DIR}/target/release/rescaleframe_overlay.dll" CACHE FILEPATH
    "Built rescaleframe_overlay.dll for the overlay DLL API device fixture")
if(EXISTS "${RSF_OVERLAY_PANEL_DLL}")
    file(TO_NATIVE_PATH "${RSF_OVERLAY_PANEL_DLL}" rsf_overlay_panel_native)
    add_test(NAME overlay_dll_api_device COMMAND rsf_overlay_dll_api_device)
    set_tests_properties(overlay_dll_api_device PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77
        ENVIRONMENT "RSF_OVERLAY_DLL=${rsf_overlay_panel_native}")
else()
    message(STATUS "overlay_dll_api_device not registered: ${RSF_OVERLAY_PANEL_DLL} is missing")
endif()
