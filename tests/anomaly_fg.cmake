# Anomaly frame generation (games/anomaly/src/fg_*). Included after anomaly_support.cmake, whose targets and vendor
# root this extends.
set(RSF_ANOMALY_FG_SRC "${PROJECT_SOURCE_DIR}/games/anomaly/src")

add_executable(rsf_anomaly_fg_policy_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_fg_policy.cpp")
target_include_directories(rsf_anomaly_fg_policy_test PRIVATE "${RSF_ANOMALY_FG_SRC}")
target_link_libraries(rsf_anomaly_fg_policy_test PRIVATE ReScaleFrame::GameSDK rsf_build_options)
add_test(NAME anomaly_fg_policy_contract COMMAND rsf_anomaly_fg_policy_test)
add_test(NAME anomaly_fg_policy_unknown_selection COMMAND rsf_anomaly_fg_policy_test unknown-case)
set_tests_properties(anomaly_fg_policy_unknown_selection PROPERTIES WILL_FAIL TRUE)
set_tests_properties(anomaly_fg_policy_contract anomaly_fg_policy_unknown_selection PROPERTIES TIMEOUT 30)

# The device fixture: the adapter's production FG stage and SR TemporalDriver against the real Runtime DLL, on the
# NVIDIA adapter, with the vendor package from RSF_ANOMALY_SR_VENDOR_ROOT (anomaly_support.cmake).
add_executable(rsf_anomaly_fg_device_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_fg_device.cpp"
    "${RSF_ANOMALY_FG_SRC}/fg_stage.cpp" "${RSF_ANOMALY_FG_SRC}/fg_inputs.cpp" "${RSF_ANOMALY_FG_SRC}/sr_temporal.cpp")
target_include_directories(rsf_anomaly_fg_device_test PRIVATE "${RSF_ANOMALY_FG_SRC}"
    "${PROJECT_SOURCE_DIR}/runtime/backends/dlss/include"
    "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
target_link_libraries(rsf_anomaly_fg_device_test PRIVATE "$<TARGET_LINKER_FILE:rsf_orchestrator>" ReScaleFrame::GameSDK
    ReScaleFrame::Contract ReScaleFrame::Graphics d3d11 dxgi dxguid rsf_build_options)
add_dependencies(rsf_anomaly_fg_device_test rsf_orchestrator)
if(RSF_ANOMALY_SR_VENDOR_ROOT AND EXISTS "${RSF_ANOMALY_SR_VENDOR_ROOT}/streamline/sl.interposer.dll")
    file(TO_NATIVE_PATH "${RSF_ANOMALY_SR_VENDOR_ROOT}" rsf_anomaly_fg_vendor_native)
    add_test(NAME anomaly_fg_dlss_sr5_device COMMAND rsf_anomaly_fg_device_test 5 1 "${rsf_anomaly_fg_vendor_native}" --switch)
    add_test(NAME anomaly_fg_dlss_sroff_device COMMAND rsf_anomaly_fg_device_test off 1 "${rsf_anomaly_fg_vendor_native}")
    add_test(NAME anomaly_fg_fsr3_sroff_device COMMAND rsf_anomaly_fg_device_test off 3 "${rsf_anomaly_fg_vendor_native}")
    add_test(NAME anomaly_fg_xess_sr5_device COMMAND rsf_anomaly_fg_device_test 5 5 "${rsf_anomaly_fg_vendor_native}")
    set_tests_properties(anomaly_fg_dlss_sr5_device anomaly_fg_dlss_sroff_device anomaly_fg_fsr3_sroff_device
        anomaly_fg_xess_sr5_device PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 300 RUN_SERIAL TRUE)
endif()

# native_observer.cpp calls the FG stage from its boundaries.
target_sources(rsf_anomaly_sr_hooks_test PRIVATE "${RSF_ANOMALY_FG_SRC}/fg_stage.cpp" "${RSF_ANOMALY_FG_SRC}/fg_inputs.cpp")
target_include_directories(rsf_anomaly_sr_hooks_test PRIVATE "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")

add_test(NAME anomaly_input_events_standard_contract COMMAND rsf_anomaly_fg_device_test --input-contract standard)
add_test(NAME anomaly_input_events_avx_contract COMMAND rsf_anomaly_fg_device_test --input-contract avx)
