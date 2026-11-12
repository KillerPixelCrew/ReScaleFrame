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
    "${RSF_ANOMALY_SR_SRC}/motion_foliage.cpp"
    "${RSF_ANOMALY_SR_SRC}/motion_policy.cpp" "${RSF_ANOMALY_SR_SRC}/motion_target.cpp" "${RSF_ANOMALY_SR_SRC}/shader_loader.cpp")
add_executable(rsf_anomaly_motion_contract_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_motion_contract.cpp" ${RSF_ANOMALY_MOTION_SOURCES})
target_include_directories(rsf_anomaly_motion_contract_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
target_compile_definitions(rsf_anomaly_motion_contract_test PRIVATE RSF_SOURCE_DIR="${PROJECT_SOURCE_DIR}")
target_link_libraries(rsf_anomaly_motion_contract_test PRIVATE bcrypt ReScaleFrame::GameSDK ReScaleFrame::Graphics rsf_build_options)
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
# Why the motion variant pair is refused under a bound hull/domain shader (first-party HLSL probe; 77 without hardware).
add_executable(rsf_anomaly_motion_tess_device_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_motion_tess_device.cpp")
target_link_libraries(rsf_anomaly_motion_tess_device_test PRIVATE d3d11 d3dcompiler dxgi dxguid rsf_build_options)
add_test(NAME anomaly_motion_tess_device COMMAND rsf_anomaly_motion_tess_device_test)
set_tests_properties(anomaly_motion_tess_device PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 60)
# The real VS+HS+DS+PS motion chain of the tessellation pass (local deployment with the dx11/tess roots, NVIDIA adapter
# preferred; 77 without deployment, d3dcompiler_43 or hardware): a skinned PN/HM mover against the CPU model.
add_executable(rsf_anomaly_motion_tess_chain_device_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_motion_tess_chain_device.cpp"
    ${RSF_ANOMALY_MOTION_SOURCES} "${RSF_ANOMALY_SR_SRC}/motion_d3d11.cpp" "${RSF_ANOMALY_SR_SRC}/shader_d3d.cpp")
target_include_directories(rsf_anomaly_motion_tess_chain_device_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
target_compile_definitions(rsf_anomaly_motion_tess_chain_device_test PRIVATE RSF_SOURCE_DIR="${PROJECT_SOURCE_DIR}")
target_link_libraries(rsf_anomaly_motion_tess_chain_device_test PRIVATE ReScaleFrame::Graphics bcrypt d3d11 d3dcompiler dxgi dxguid
    rsf_build_options)
add_test(NAME anomaly_motion_tess_chain_device COMMAND rsf_anomaly_motion_tess_chain_device_test)
set_tests_properties(anomaly_motion_tess_chain_device PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 300)
# The far (SE_R2_NORMAL_LQ, beyond r__dtex_range) and near (HQ) skinned VS/PS pairs against the CPU model, including a
# few-pixel mover 100 m away (local deployment, NVIDIA adapter preferred; 77 without deployment, d3dcompiler_43 or hardware).
add_executable(rsf_anomaly_motion_far_device_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_motion_far_device.cpp"
    ${RSF_ANOMALY_MOTION_SOURCES} "${RSF_ANOMALY_SR_SRC}/motion_d3d11.cpp" "${RSF_ANOMALY_SR_SRC}/shader_d3d.cpp")
target_include_directories(rsf_anomaly_motion_far_device_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
target_compile_definitions(rsf_anomaly_motion_far_device_test PRIVATE RSF_SOURCE_DIR="${PROJECT_SOURCE_DIR}")
target_link_libraries(rsf_anomaly_motion_far_device_test PRIVATE ReScaleFrame::Graphics bcrypt d3d11 d3dcompiler dxgi dxguid
    rsf_build_options)
add_test(NAME anomaly_motion_far_device COMMAND rsf_anomaly_motion_far_device_test)
set_tests_properties(anomaly_motion_far_device PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 300)
# rsf_anomaly_sr_hooks_test links native_observer.cpp, which installs/reports/quiesces the motion stage.
target_sources(rsf_anomaly_sr_hooks_test PRIVATE ${RSF_ANOMALY_MOTION_SOURCES} "${RSF_ANOMALY_SR_SRC}/motion_d3d11.cpp"
    "${RSF_ANOMALY_SR_SRC}/motion_stage.cpp" "${RSF_ANOMALY_SR_SRC}/shader_d3d.cpp")
target_link_libraries(rsf_anomaly_sr_hooks_test PRIVATE bcrypt dxguid d3dcompiler)
# SR Stage 3 (jitter and mip bias). The jitter test checks the Halton sequence, the row-vector NDC offset, the
# first-person rule, the mip bias delta and the published motion/SR state on the CPU; the Stage 3 swap cases (projection
# and bias restore on every exit path) live in anomaly_sr_swap_contract. The device fixture runs the unchanged core
# motion PS writer with production b13 bytes and jitter (77 when no D3D11 device exists). None of them runs game code.
add_executable(rsf_anomaly_sr_jitter_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_jitter.cpp")
target_include_directories(rsf_anomaly_sr_jitter_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
target_link_libraries(rsf_anomaly_sr_jitter_test PRIVATE rsf_build_options)
add_test(NAME anomaly_sr_jitter_contract COMMAND rsf_anomaly_sr_jitter_test)
add_test(NAME anomaly_sr_jitter_unknown_selection COMMAND rsf_anomaly_sr_jitter_test unknown-case)
set_tests_properties(anomaly_sr_jitter_unknown_selection PROPERTIES WILL_FAIL TRUE)
add_executable(rsf_anomaly_sr_jitter_device_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_jitter_device.cpp"
    "${RSF_ANOMALY_SR_SRC}/motion_constants.cpp")
target_include_directories(rsf_anomaly_sr_jitter_device_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
target_compile_definitions(rsf_anomaly_sr_jitter_device_test PRIVATE RSF_SOURCE_DIR="${PROJECT_SOURCE_DIR}")
target_link_libraries(rsf_anomaly_sr_jitter_device_test PRIVATE d3d11 d3dcompiler dxgi dxguid rsf_build_options)
add_test(NAME anomaly_sr_jitter_device COMMAND rsf_anomaly_sr_jitter_device_test)
set_tests_properties(anomaly_sr_jitter_device PROPERTIES SKIP_RETURN_CODE 77)
set_tests_properties(anomaly_sr_jitter_contract anomaly_sr_jitter_unknown_selection anomaly_sr_jitter_device PROPERTIES TIMEOUT 120)
# The Stage 3 burst measurement (tests/anomaly_sr_evidence.py --measure-jitter) on a synthetic shifted burst; 77 without numpy.
if(Python3_Interpreter_FOUND)
    add_test(NAME anomaly_sr_measure_self_test COMMAND "${Python3_EXECUTABLE}" -I "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_evidence.py"
        --measure-self-test)
    set_tests_properties(anomaly_sr_measure_self_test PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)
endif()
# SR Stages 4 and 5 (section 12). The pass contract checks sr_pass.h/sr_plan.h on the CPU. The Runtime API contract is a
# client that imports ReScaleFrame.Runtime.dll like a carrier-loaded adapter and checks rsf_get_d3d11_sr_api plus the
# adapter's table validation. The device fixture runs the production decode/resolve parameters and the prepare and
# reinsert passes on any D3D11 device; its provider case needs the vendor runtimes in the package layout
# (<root>/{streamline,fidelityfx,xess}) and an NVIDIA adapter, and is registered when RSF_ANOMALY_SR_VENDOR_ROOT exists.
add_executable(rsf_anomaly_sr_pass_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_pass.cpp")
target_include_directories(rsf_anomaly_sr_pass_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
target_link_libraries(rsf_anomaly_sr_pass_test PRIVATE ReScaleFrame::GameSDK ReScaleFrame::Graphics rsf_build_options)
add_test(NAME anomaly_sr_pass_contract COMMAND rsf_anomaly_sr_pass_test)
add_test(NAME anomaly_sr_pass_unknown_selection COMMAND rsf_anomaly_sr_pass_test unknown-case)
set_tests_properties(anomaly_sr_pass_unknown_selection PROPERTIES WILL_FAIL TRUE)
foreach(rsf_sr5_client rsf_anomaly_sr_runtime_api_test rsf_anomaly_sr_temporal_device_test)
    if(rsf_sr5_client STREQUAL "rsf_anomaly_sr_runtime_api_test")
        add_executable(${rsf_sr5_client} "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_runtime_api.cpp" "${RSF_ANOMALY_SR_SRC}/sr_temporal.cpp")
    else()
        add_executable(${rsf_sr5_client} "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_temporal_device.cpp" "${RSF_ANOMALY_SR_SRC}/sr_temporal.cpp"
            "${RSF_ANOMALY_SR_SRC}/sr_d3d11.cpp")
        target_link_libraries(${rsf_sr5_client} PRIVATE ReScaleFrame::Graphics rsf_anomaly_sr_shaders d3d11 dxgi d3dcompiler)
    endif()
    target_include_directories(${rsf_sr5_client} PRIVATE "${RSF_ANOMALY_SR_SRC}"
        "${PROJECT_SOURCE_DIR}/runtime/backends/dlss/include"
        "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
    target_link_libraries(${rsf_sr5_client} PRIVATE "$<TARGET_LINKER_FILE:rsf_orchestrator>" ReScaleFrame::GameSDK ReScaleFrame::Contract
        rsf_build_options)
    add_dependencies(${rsf_sr5_client} rsf_orchestrator)
endforeach()
add_test(NAME anomaly_sr_runtime_api_contract COMMAND rsf_anomaly_sr_runtime_api_test)
add_test(NAME anomaly_sr_temporal_device COMMAND rsf_anomaly_sr_temporal_device_test)
set_tests_properties(anomaly_sr_temporal_device PROPERTIES SKIP_RETURN_CODE 77)
file(GLOB rsf_anomaly_sr_vendor_candidates LIST_DIRECTORIES true "${PROJECT_SOURCE_DIR}/build/releases/rdr2/stage-rdr2-*/ReScaleFrame")
list(SORT rsf_anomaly_sr_vendor_candidates)
list(LENGTH rsf_anomaly_sr_vendor_candidates rsf_anomaly_sr_vendor_count)
set(rsf_anomaly_sr_vendor_default "")
if(rsf_anomaly_sr_vendor_count GREATER 0)
    list(GET rsf_anomaly_sr_vendor_candidates -1 rsf_anomaly_sr_vendor_default)
endif()
set(RSF_ANOMALY_SR_VENDOR_ROOT "${rsf_anomaly_sr_vendor_default}" CACHE PATH
    "Package layout with streamline/, fidelityfx/ and xess/ for the Anomaly SR provider fixture")
if(RSF_ANOMALY_SR_VENDOR_ROOT AND EXISTS "${RSF_ANOMALY_SR_VENDOR_ROOT}/streamline/sl.interposer.dll")
    file(TO_NATIVE_PATH "${RSF_ANOMALY_SR_VENDOR_ROOT}" rsf_anomaly_sr_vendor_native)
    add_test(NAME anomaly_sr_provider_device COMMAND rsf_anomaly_sr_temporal_device_test provider "${rsf_anomaly_sr_vendor_native}")
    set_tests_properties(anomaly_sr_provider_device PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 300)
endif()
# Backend switching through the shared pipeline on hardware (generic runtime behaviour, driven by the same vendor root).
add_executable(rsf_sr_backend_switch_device_test "${CMAKE_CURRENT_LIST_DIR}/sr_backend_switch_device.cpp")
target_link_libraries(rsf_sr_backend_switch_device_test PRIVATE rsf_orchestrator rsf_build_options)
if(RSF_ANOMALY_SR_VENDOR_ROOT AND EXISTS "${RSF_ANOMALY_SR_VENDOR_ROOT}/streamline/sl.interposer.dll")
    foreach(rsf_switch_mode baseline select select+exposure)
        add_test(NAME sr_backend_switch_${rsf_switch_mode}
            COMMAND rsf_sr_backend_switch_device_test "${rsf_anomaly_sr_vendor_native}" ${rsf_switch_mode})
        set_tests_properties(sr_backend_switch_${rsf_switch_mode} PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 600)
    endforeach()
endif()
set_tests_properties(anomaly_sr_pass_contract anomaly_sr_pass_unknown_selection anomaly_sr_runtime_api_contract
    anomaly_sr_temporal_device PROPERTIES TIMEOUT 120)
# Stage 5 is part of sr_stage.cpp, which the hooks contract links.
target_sources(rsf_anomaly_sr_hooks_test PRIVATE "${RSF_ANOMALY_SR_SRC}/sr_temporal.cpp")
target_include_directories(rsf_anomaly_sr_hooks_test PRIVATE "${PROJECT_SOURCE_DIR}/runtime/backends/dlss/include")

# Profile-dependent fixtures once more against the AVX executable's tables (build_profile.h selects them by name and
# SHA-256 in the game; here the argument selects them). anomaly_sr_profile_contract checks both tables at compile time.
add_test(NAME anomaly_geometry_hooks_avx_contract COMMAND rsf_anomaly_geometry_hooks_test avx)
add_test(NAME anomaly_sr_hooks_avx_contract COMMAND rsf_anomaly_sr_hooks_test avx)
set_tests_properties(anomaly_geometry_hooks_avx_contract anomaly_sr_hooks_avx_contract PROPERTIES TIMEOUT 120)

# Shadow-frustum jitter suspension and restoration, including fault rollback and no-jitter paths.
add_test(NAME anomaly_sr_sun_cascades_contract COMMAND rsf_anomaly_sr_swap_test stage3_sun_cascades)
set_tests_properties(anomaly_sr_sun_cascades_contract PROPERTIES TIMEOUT 30)

# Jitter-only receiver-depth margin: base ownership, live console edits, exits and partial writes.
add_test(NAME anomaly_sr_sun_depth_margin_contract COMMAND rsf_anomaly_sr_swap_test stage3_sun_depth_margin)
set_tests_properties(anomaly_sr_sun_depth_margin_contract PROPERTIES TIMEOUT 30)

target_sources(rsf_anomaly_sr_hooks_test PRIVATE "${RSF_ANOMALY_SR_SRC}/runtime_shaders.cpp")
