# CPU history/planning contracts only; these tests do not prove game motion output.
add_executable(rsf_anomaly_temporal_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_temporal_tests.cpp")
target_include_directories(rsf_anomaly_temporal_test PRIVATE "${PROJECT_SOURCE_DIR}")
target_link_libraries(rsf_anomaly_temporal_test PRIVATE rsf_build_options)
add_test(NAME anomaly_temporal_contract COMMAND rsf_anomaly_temporal_test)
add_test(NAME anomaly_temporal_unknown_selection COMMAND rsf_anomaly_temporal_test unknown-case)
set_tests_properties(anomaly_temporal_unknown_selection PROPERTIES WILL_FAIL TRUE)
set_tests_properties(anomaly_temporal_contract anomaly_temporal_unknown_selection PROPERTIES TIMEOUT 30)

find_package(Python3 COMPONENTS Interpreter QUIET)
if(Python3_Interpreter_FOUND)
    add_test(NAME anomaly_shader_tool_contract COMMAND "${Python3_EXECUTABLE}" -m unittest discover
        -s "${CMAKE_CURRENT_LIST_DIR}" -p anomaly_shader_test.py -v)
    set_tests_properties(anomaly_shader_tool_contract PROPERTIES
        WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}" TIMEOUT 120)
endif()

# Geometry identity/lifetime producer. Pure CPU registry, draw scopes and history feed, then real x64
# detours on fixture functions plus the production guarded install against a synthetic image. No game code runs.
set(RSF_ANOMALY_GEOMETRY_SOURCES
    "${PROJECT_SOURCE_DIR}/games/anomaly/src/native_geometry.cpp"
    "${PROJECT_SOURCE_DIR}/games/anomaly/src/geometry_feed.cpp"
    "${PROJECT_SOURCE_DIR}/games/anomaly/src/native_hooks.cpp")
add_executable(rsf_anomaly_geometry_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_geometry_tests.cpp" ${RSF_ANOMALY_GEOMETRY_SOURCES})
target_include_directories(rsf_anomaly_geometry_test PRIVATE "${PROJECT_SOURCE_DIR}/games/anomaly/src")
target_link_libraries(rsf_anomaly_geometry_test PRIVATE minhook rsf_build_options)
add_test(NAME anomaly_geometry_contract COMMAND rsf_anomaly_geometry_test)
add_executable(rsf_anomaly_geometry_hooks_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_geometry_hooks.cpp" ${RSF_ANOMALY_GEOMETRY_SOURCES})
target_include_directories(rsf_anomaly_geometry_hooks_test PRIVATE "${PROJECT_SOURCE_DIR}/games/anomaly/src")
target_compile_definitions(rsf_anomaly_geometry_hooks_test PRIVATE RSF_ANOMALY_GEOMETRY_TESTING)
target_link_libraries(rsf_anomaly_geometry_hooks_test PRIVATE minhook rsf_build_options)
add_test(NAME anomaly_geometry_hooks_contract COMMAND rsf_anomaly_geometry_hooks_test)
set_tests_properties(anomaly_geometry_contract anomaly_geometry_hooks_contract PROPERTIES TIMEOUT 60)

# Shared-overlay wiring for Anomaly. The gate test uses a synthetic IDirectInputDevice8A; the API test uses fake
# Runtime tables; the hooks test drives the production start() against a WARP swap chain and the system
# DirectInput mouse/keyboard (skips with 77 where those do not exist). None of them loads the real Runtime,
# the panel or the game, so they establish contracts, not an in-game acceptance.
set(RSF_ANOMALY_OVERLAY_SOURCES
    "${PROJECT_SOURCE_DIR}/games/anomaly/src/overlay_input_gate.cpp"
    "${PROJECT_SOURCE_DIR}/games/anomaly/src/overlay_hooks.cpp"
    "${PROJECT_SOURCE_DIR}/games/anomaly/src/overlay_service.cpp")
foreach(overlay_test IN ITEMS input_gate api hooks)
    add_executable(rsf_anomaly_overlay_${overlay_test}_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_overlay_${overlay_test}.cpp"
        ${RSF_ANOMALY_OVERLAY_SOURCES})
    target_include_directories(rsf_anomaly_overlay_${overlay_test}_test PRIVATE
        "${PROJECT_SOURCE_DIR}/games/anomaly/src" "${PROJECT_SOURCE_DIR}/runtime/orchestrator/include")
    target_link_libraries(rsf_anomaly_overlay_${overlay_test}_test PRIVATE
        ReScaleFrame::GameSDK ReScaleFrame::Contract minhook rsf_build_options)
    add_test(NAME anomaly_overlay_${overlay_test}_contract COMMAND rsf_anomaly_overlay_${overlay_test}_test)
    set_tests_properties(anomaly_overlay_${overlay_test}_contract PROPERTIES TIMEOUT 60)
endforeach()
target_link_libraries(rsf_anomaly_overlay_hooks_test PRIVATE d3d11 dxgi dinput8 dxguid)
set_tests_properties(anomaly_overlay_hooks_contract PROPERTIES SKIP_RETURN_CODE 77)

# SR Stage 0 site profile: compile-time consistency only. The guards are checked against the installed executable by
# tests/anomaly_sr_evidence.py --verify, which is not registered here because it needs the game install.
add_executable(rsf_anomaly_sr_profile_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_profile.cpp")
target_include_directories(rsf_anomaly_sr_profile_test PRIVATE "${PROJECT_SOURCE_DIR}/games/anomaly/src")
target_link_libraries(rsf_anomaly_sr_profile_test PRIVATE rsf_build_options)
add_test(NAME anomaly_sr_profile_contract COMMAND rsf_anomaly_sr_profile_test)
set_tests_properties(anomaly_sr_profile_contract PROPERTIES TIMEOUT 30)

# SR Stage 1 (scale-1.0 twin swap). The swap test drives the real state machine on fake CRT/CTexture/RCache memory
# laid out with sr_profile.h offsets; the hooks test checks the shared rmNear slot and every install refusal on a
# synthetic image; the round-trip fixture runs the production pass and shader on a real D3D11 device (hardware
# with the debug layer when present, else WARP; 77 when neither exists). None of them runs game code.
set(RSF_ANOMALY_SR_SRC "${PROJECT_SOURCE_DIR}/games/anomaly/src")
add_executable(rsf_anomaly_sr_swap_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_swap.cpp" "${RSF_ANOMALY_SR_SRC}/sr_swap.cpp")
target_include_directories(rsf_anomaly_sr_swap_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
target_link_libraries(rsf_anomaly_sr_swap_test PRIVATE rsf_build_options)
add_test(NAME anomaly_sr_swap_contract COMMAND rsf_anomaly_sr_swap_test)
add_test(NAME anomaly_sr_swap_unknown_selection COMMAND rsf_anomaly_sr_swap_test unknown-case)
set_tests_properties(anomaly_sr_swap_unknown_selection PROPERTIES WILL_FAIL TRUE)
add_executable(rsf_anomaly_sr_hooks_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_hooks.cpp"
    "${RSF_ANOMALY_SR_SRC}/sr_stage.cpp" "${RSF_ANOMALY_SR_SRC}/sr_swap.cpp" "${RSF_ANOMALY_SR_SRC}/sr_d3d11.cpp"
    "${RSF_ANOMALY_SR_SRC}/native_observer.cpp" ${RSF_ANOMALY_GEOMETRY_SOURCES})
target_include_directories(rsf_anomaly_sr_hooks_test PRIVATE "${RSF_ANOMALY_SR_SRC}" "${PROJECT_SOURCE_DIR}/runtime/orchestrator/include")
target_link_libraries(rsf_anomaly_sr_hooks_test PRIVATE ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::RuntimeCommon
    ReScaleFrame::Graphics rsf_anomaly_sr_shaders minhook d3d11 rsf_build_options)
add_test(NAME anomaly_sr_hooks_contract COMMAND rsf_anomaly_sr_hooks_test)
add_executable(rsf_anomaly_sr_roundtrip_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_roundtrip.cpp" "${RSF_ANOMALY_SR_SRC}/sr_d3d11.cpp")
target_include_directories(rsf_anomaly_sr_roundtrip_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
target_link_libraries(rsf_anomaly_sr_roundtrip_test PRIVATE ReScaleFrame::Graphics rsf_anomaly_sr_shaders d3d11 dxgi rsf_build_options)
add_test(NAME anomaly_sr_roundtrip_device COMMAND rsf_anomaly_sr_roundtrip_test)
set_tests_properties(anomaly_sr_roundtrip_device PROPERTIES SKIP_RETURN_CODE 77)
set_tests_properties(anomaly_sr_swap_contract anomaly_sr_swap_unknown_selection anomaly_sr_hooks_contract
    anomaly_sr_roundtrip_device PROPERTIES TIMEOUT 120)

# SR Stage 2 (render-size world window). The plan test checks the render/output arithmetic; the Stage 2 swap cases live
# in anomaly_sr_swap_contract and the upscale/depth-twin fixture in anomaly_sr_roundtrip_device.
add_executable(rsf_anomaly_sr_plan_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_plan.cpp")
target_include_directories(rsf_anomaly_sr_plan_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
target_link_libraries(rsf_anomaly_sr_plan_test PRIVATE rsf_build_options)
add_test(NAME anomaly_sr_plan_contract COMMAND rsf_anomaly_sr_plan_test)
set_tests_properties(anomaly_sr_plan_contract PROPERTIES TIMEOUT 30)

# Motion MRT. The contract test checks the b13 layouts against the shader READMEs/headers, the history policy, the
# RTV2 bind-preservation state machine on fake RCache memory and the shader-loader hook contract (caller frame,
# manifest, original-bytecode proof, reflection rules, disk cache) with fakes. The device fixture draws the real
# deployed core variants (tools/anomaly_motion_deploy.py output, local only; 77 when absent) through the production
# loader, b13 packers and invalidation replay. Neither runs game code.
set(RSF_ANOMALY_MOTION_SOURCES "${RSF_ANOMALY_SR_SRC}/motion_constants.cpp" "${RSF_ANOMALY_SR_SRC}/motion_history.cpp"
    "${RSF_ANOMALY_SR_SRC}/motion_policy.cpp" "${RSF_ANOMALY_SR_SRC}/motion_target.cpp" "${RSF_ANOMALY_SR_SRC}/shader_loader.cpp")
add_executable(rsf_anomaly_motion_contract_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_motion_contract.cpp" ${RSF_ANOMALY_MOTION_SOURCES})
target_include_directories(rsf_anomaly_motion_contract_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
target_compile_definitions(rsf_anomaly_motion_contract_test PRIVATE RSF_SOURCE_DIR="${PROJECT_SOURCE_DIR}")
target_link_libraries(rsf_anomaly_motion_contract_test PRIVATE bcrypt rsf_build_options)
add_test(NAME anomaly_motion_contract COMMAND rsf_anomaly_motion_contract_test)
add_test(NAME anomaly_motion_unknown_selection COMMAND rsf_anomaly_motion_contract_test unknown-case)
set_tests_properties(anomaly_motion_unknown_selection PROPERTIES WILL_FAIL TRUE)
add_executable(rsf_anomaly_motion_device_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_motion_device.cpp" ${RSF_ANOMALY_MOTION_SOURCES}
    "${RSF_ANOMALY_SR_SRC}/motion_d3d11.cpp" "${RSF_ANOMALY_SR_SRC}/shader_d3d.cpp")
target_include_directories(rsf_anomaly_motion_device_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
target_compile_definitions(rsf_anomaly_motion_device_test PRIVATE RSF_SOURCE_DIR="${PROJECT_SOURCE_DIR}")
target_link_libraries(rsf_anomaly_motion_device_test PRIVATE ReScaleFrame::Graphics bcrypt d3d11 d3dcompiler dxgi dxguid rsf_build_options)
add_test(NAME anomaly_motion_device COMMAND rsf_anomaly_motion_device_test)
set_tests_properties(anomaly_motion_device PROPERTIES SKIP_RETURN_CODE 77)
set_tests_properties(anomaly_motion_contract anomaly_motion_unknown_selection anomaly_motion_device PROPERTIES TIMEOUT 300)
# The geometry flush detour (redirected onto a fixture) calls the motion stage's flush slot after the original.
add_executable(rsf_anomaly_motion_hooks_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_motion_hooks.cpp" ${RSF_ANOMALY_GEOMETRY_SOURCES})
target_include_directories(rsf_anomaly_motion_hooks_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
target_link_libraries(rsf_anomaly_motion_hooks_test PRIVATE minhook rsf_build_options)
add_test(NAME anomaly_motion_hooks_contract COMMAND rsf_anomaly_motion_hooks_test)
set_tests_properties(anomaly_motion_hooks_contract PROPERTIES TIMEOUT 60)
# rsf_anomaly_sr_hooks_test links native_observer.cpp, which installs/reports/quiesces the motion stage.
target_sources(rsf_anomaly_sr_hooks_test PRIVATE ${RSF_ANOMALY_MOTION_SOURCES} "${RSF_ANOMALY_SR_SRC}/motion_d3d11.cpp"
    "${RSF_ANOMALY_SR_SRC}/motion_stage.cpp" "${RSF_ANOMALY_SR_SRC}/shader_d3d.cpp")
target_link_libraries(rsf_anomaly_sr_hooks_test PRIVATE bcrypt dxguid d3dcompiler)
