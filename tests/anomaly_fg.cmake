# Anomaly frame generation (games/anomaly/src/fg_*). Included after anomaly_support.cmake, whose targets and vendor
# root this extends.
set(RSF_ANOMALY_FG_SRC "${PROJECT_SOURCE_DIR}/games/anomaly/src")

# The device fixture: the adapter's production FG stage and SR TemporalDriver against the real Runtime DLL, on the
# NVIDIA adapter, with the vendor package from RSF_ANOMALY_SR_VENDOR_ROOT (anomaly_support.cmake).
add_executable(rsf_anomaly_fg_device_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_fg_device.c")
set_target_properties(rsf_anomaly_fg_device_test PROPERTIES LINKER_LANGUAGE CXX)
target_include_directories(rsf_anomaly_fg_device_test PRIVATE "${RSF_ANOMALY_FG_SRC}"
    "${PROJECT_SOURCE_DIR}/runtime/backends/dlss/include"
    "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
target_link_libraries(rsf_anomaly_fg_device_test PRIVATE rsf_anomaly_fixtures_rust
    "$<TARGET_LINKER_FILE:rsf_orchestrator>" ReScaleFrame::GameSDK
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

add_test(NAME anomaly_input_events_standard_contract COMMAND rsf_anomaly_fg_device_test --input-contract standard)
add_test(NAME anomaly_input_events_avx_contract COMMAND rsf_anomaly_fg_device_test --input-contract avx)
