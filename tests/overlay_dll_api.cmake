# Clients of rsf_get_overlay_api that import ReScaleFrame.Runtime.dll the way a DLL-loaded
# adapter does. Keep them free of OrchestratorCore and its public RSF_RUNTIME_BUILD define.
add_executable(rsf_overlay_dll_api_test "${CMAKE_CURRENT_LIST_DIR}/overlay_dll_api.cpp")
add_executable(rsf_overlay_dll_api_c11 "${CMAKE_CURRENT_LIST_DIR}/overlay_dll_api_c11.c")
add_executable(rsf_overlay_dll_api_device "${CMAKE_CURRENT_LIST_DIR}/overlay_dll_api_device.cpp")
add_executable(rsf_overlay_dll_api_no_sr "${CMAKE_CURRENT_LIST_DIR}/overlay_dll_api_no_sr.cpp")
set_target_properties(rsf_overlay_dll_api_c11 PROPERTIES
    C_STANDARD 11 C_STANDARD_REQUIRED ON C_EXTENSIONS OFF)
foreach(client rsf_overlay_dll_api_test rsf_overlay_dll_api_c11 rsf_overlay_dll_api_device rsf_overlay_dll_api_no_sr)
    target_include_directories(${client} PRIVATE
        "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
    target_link_libraries(${client} PRIVATE "$<TARGET_LINKER_FILE:rsf_orchestrator>" rsf_build_options)
    add_dependencies(${client} rsf_orchestrator)
endforeach()
target_link_libraries(rsf_overlay_dll_api_device PRIVATE d3d11 dxgi)
add_test(NAME overlay_dll_api_contract COMMAND rsf_overlay_dll_api_test)
add_test(NAME overlay_dll_api_c11_contract COMMAND rsf_overlay_dll_api_c11)
add_test(NAME overlay_dll_api_no_sr_contract COMMAND rsf_overlay_dll_api_no_sr)
set_tests_properties(overlay_dll_api_contract overlay_dll_api_c11_contract overlay_dll_api_no_sr_contract PROPERTIES TIMEOUT 30)

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

# The Anomaly adapter's evidence path against the real Runtime and panel: the backbuffer capture PNGs
# contain panel pixels while open, and the adapter log reports drawn state, pointer events and settings
# requests. The adapter service is compiled in (hooks off) and imports the Runtime like a carrier would.
if(EXISTS "${RSF_OVERLAY_PANEL_DLL}")
    add_executable(rsf_anomaly_overlay_visibility_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_overlay_visibility.cpp"
        "${PROJECT_SOURCE_DIR}/games/anomaly/src/overlay_input_gate.cpp"
        "${PROJECT_SOURCE_DIR}/games/anomaly/src/overlay_hooks.cpp"
        "${PROJECT_SOURCE_DIR}/games/anomaly/src/overlay_service.cpp")
    target_include_directories(rsf_anomaly_overlay_visibility_test PRIVATE
        "${PROJECT_SOURCE_DIR}/games/anomaly/src"
        "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
    target_link_libraries(rsf_anomaly_overlay_visibility_test PRIVATE "$<TARGET_LINKER_FILE:rsf_orchestrator>"
        ReScaleFrame::GameSDK ReScaleFrame::Contract minhook rsf_build_options d3d11 dxgi dinput8 dxguid windowscodecs ole32)
    add_dependencies(rsf_anomaly_overlay_visibility_test rsf_orchestrator)
    add_test(NAME anomaly_overlay_visibility COMMAND rsf_anomaly_overlay_visibility_test)
    set_tests_properties(anomaly_overlay_visibility PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77
        ENVIRONMENT "RSF_OVERLAY_DLL=${rsf_overlay_panel_native}")
endif()
