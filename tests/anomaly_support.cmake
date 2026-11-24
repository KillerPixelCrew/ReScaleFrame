
# CPU history/planning contracts only; these tests do not prove game motion output.
rsf_add_fixture_test(rsf_anomaly_temporal_test NAME anomaly_temporal_contract ENTRY rsf_anomaly_temporal_planning_fixture ARGV
    LIBS rsf_anomaly_fixtures_rust TIMEOUT 30)
target_include_directories(rsf_anomaly_temporal_test PRIVATE "${PROJECT_SOURCE_DIR}")

find_package(Python3 COMPONENTS Interpreter QUIET)
# Every *_test.py here covers a script in tools/. The Anomaly shader tests need the licensed local
# corpus and skip without it.
if(Python3_Interpreter_FOUND)
    add_test(NAME python_tool_tests COMMAND "${Python3_EXECUTABLE}" -I -m unittest discover
        -s "${CMAKE_CURRENT_LIST_DIR}" -p "*_test.py")
    set_tests_properties(python_tool_tests PROPERTIES WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}" TIMEOUT 300)
endif()

# Geometry identity/lifetime producer. Pure CPU registry, draw scopes and history feed, then real x64
# detours on fixture functions plus the production guarded install against a synthetic image. No game code runs.
# The adapter's C leaves come with the fixture archive (rsf_anomaly_native_leaves).
add_executable(rsf_anomaly_geometry_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_geometry_tests.c")
target_link_libraries(rsf_anomaly_geometry_test PRIVATE rsf_anomaly_fixtures_rust minhook rsf_build_options)
add_test(NAME anomaly_geometry_contract COMMAND rsf_anomaly_geometry_test)
add_executable(rsf_anomaly_geometry_hooks_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_geometry_hooks.c")
target_compile_definitions(rsf_anomaly_geometry_hooks_test PRIVATE RSF_ANOMALY_GEOMETRY_TESTING)
target_link_libraries(rsf_anomaly_geometry_hooks_test PRIVATE rsf_anomaly_fixtures_rust ReScaleFrame::GameSDK minhook rsf_build_options)
add_test(NAME anomaly_geometry_hooks_contract COMMAND rsf_anomaly_geometry_hooks_test)
set_tests_properties(anomaly_geometry_contract anomaly_geometry_hooks_contract PROPERTIES TIMEOUT 60)

# Rust unit fixtures cover the input gate and overlay API contracts. This fixture uses the
# Windows SDK to exercise the installed hooks through WARP and system DirectInput.
add_executable(rsf_anomaly_overlay_hooks_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_overlay_hooks.c")
set_target_properties(rsf_anomaly_overlay_hooks_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON)
target_include_directories(rsf_anomaly_overlay_hooks_test PRIVATE
    "${PROJECT_SOURCE_DIR}/games/anomaly/src" "${PROJECT_SOURCE_DIR}/runtime/orchestrator/include")
target_link_libraries(rsf_anomaly_overlay_hooks_test PRIVATE
    rsf_anomaly_fixtures_rust ReScaleFrame::GameSDK ReScaleFrame::Contract minhook rsf_build_options d3d11 dxgi dinput8 dxguid)
add_test(NAME anomaly_overlay_hooks_contract COMMAND rsf_anomaly_overlay_hooks_test)
set_tests_properties(anomaly_overlay_hooks_contract PROPERTIES TIMEOUT 60 SKIP_RETURN_CODE 77)

# SR Stage 0 site profile: compile-time consistency and runtime build selection. The guards are checked against the installed executable by
# tools/anomaly_sr_evidence.py --verify, which is not registered here because it needs the game install.
rsf_add_fixture_test(rsf_anomaly_sr_profile_test NAME anomaly_sr_profile_contract ENTRY rsf_test_anomaly_profile_contract
    LIBS rsf_anomaly_fixtures_rust TIMEOUT 30)
target_include_directories(rsf_anomaly_sr_profile_test PRIVATE "${PROJECT_SOURCE_DIR}/games/anomaly/src")

# SR Stage 1 (scale-1.0 twin swap). The swap owner's cases on fake CRT/CTexture/RCache memory laid out with the
# sr_profile.rs offsets are cargo tests (games/anomaly/src/sr_swap/tests); the hooks test checks the shared rmNear slot and every install refusal on a
# synthetic image; the round-trip fixture runs the production pass and shader on a real D3D11 device (hardware
# with the debug layer when present, else WARP; 77 when neither exists). None of them runs game code.
set(RSF_ANOMALY_SR_SRC "${PROJECT_SOURCE_DIR}/games/anomaly/src")
add_executable(rsf_anomaly_sr_hooks_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_hook_bridge.c")
target_link_libraries(rsf_anomaly_sr_hooks_test PRIVATE rsf_anomaly_fixtures_rust rsf_build_options)
add_test(NAME anomaly_sr_hooks_contract COMMAND rsf_anomaly_sr_hooks_test)
add_executable(rsf_anomaly_sr_roundtrip_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_roundtrip.c")
target_link_libraries(rsf_anomaly_sr_roundtrip_test PRIVATE rsf_anomaly_fixtures_rust
    ReScaleFrame::Graphics d3d11 dxgi rsf_build_options)
add_test(NAME anomaly_sr_roundtrip_device COMMAND rsf_anomaly_sr_roundtrip_test)
set_tests_properties(anomaly_sr_roundtrip_device PROPERTIES SKIP_RETURN_CODE 77)
set_tests_properties(anomaly_sr_hooks_contract
    anomaly_sr_roundtrip_device PROPERTIES TIMEOUT 120)

# Motion MRT. The contract test checks the b13 layouts against the shader READMEs/headers, the history policy, the
# RTV2 bind-preservation state machine on fake RCache memory and the shader-loader hook contract (caller frame,
# manifest, original-bytecode proof, reflection rules, disk cache) with fakes. The device fixture draws the real
# deployed core variants (tools/anomaly_motion_deploy.py output, local only; 77 when absent) through the Rust
# deployed-tree loader, b13 packers and invalidation replay. Neither runs game code.
# The device fixtures are Rust (motion/device_fixture/*_device.rs) behind C11 launchers.
rsf_add_fixture_test(rsf_anomaly_motion_contract_test NAME anomaly_motion_contract ENTRY rsf_anomaly_motion_contract ARGV CXX
    LIBS rsf_anomaly_fixtures_rust bcrypt ReScaleFrame::GameSDK ReScaleFrame::Graphics TIMEOUT 300)
rsf_add_fixture_test(rsf_anomaly_motion_device_test NAME anomaly_motion_device ENTRY rsf_anomaly_motion_device_fixture CXX
    LIBS rsf_anomaly_fixtures_rust ReScaleFrame::Graphics bcrypt d3d11 d3dcompiler dxgi dxguid SKIP77 TIMEOUT 300)
# The geometry flush detour (redirected onto a fixture) calls the motion stage's flush slot after the original.
add_executable(rsf_anomaly_motion_hooks_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_motion_hooks_native.c")
target_link_libraries(rsf_anomaly_motion_hooks_test PRIVATE rsf_anomaly_fixtures_rust
    minhook rsf_build_options)
add_test(NAME anomaly_motion_hooks_contract COMMAND rsf_anomaly_motion_hooks_test)
set_tests_properties(anomaly_motion_hooks_contract PROPERTIES TIMEOUT 60)
# Why the motion variant pair is refused under a bound hull/domain shader (first-party HLSL probe; 77 without hardware).
rsf_add_rust_artifacts(rsf_anomaly_motion_tess_device_test rsf-anomaly "" ALL
    FILES examples/anomaly_motion_tess_device.exe ARGS --example anomaly_motion_tess_device)
add_test(NAME anomaly_motion_tess_device COMMAND ${CMAKE_CROSSCOMPILING_EMULATOR}
    "${RSF_CARGO_ARTIFACTS}/examples/anomaly_motion_tess_device.exe")
set_tests_properties(anomaly_motion_tess_device PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 60)
# The real VS+HS+DS+PS motion chain of the tessellation pass (local deployment with the dx11/tess roots, NVIDIA adapter
# preferred; 77 without deployment, d3dcompiler_43 or hardware): a skinned PN/HM mover against the CPU model.
rsf_add_fixture_test(rsf_anomaly_motion_tess_chain_device_test NAME anomaly_motion_tess_chain_device
    ENTRY rsf_anomaly_motion_tess_chain_device_fixture CXX
    LIBS rsf_anomaly_fixtures_rust ReScaleFrame::Graphics bcrypt d3d11 d3dcompiler dxgi dxguid SKIP77 TIMEOUT 300)
# The far (SE_R2_NORMAL_LQ, beyond r__dtex_range) and near (HQ) skinned VS/PS pairs against the CPU model, including a
# few-pixel mover 100 m away (local deployment, NVIDIA adapter preferred; 77 without deployment, d3dcompiler_43 or hardware).
rsf_add_fixture_test(rsf_anomaly_motion_far_device_test NAME anomaly_motion_far_device
    ENTRY rsf_anomaly_motion_far_device_fixture CXX
    LIBS rsf_anomaly_fixtures_rust ReScaleFrame::Graphics bcrypt d3d11 d3dcompiler dxgi dxguid SKIP77 TIMEOUT 300)
# rsf_anomaly_sr_hooks_test runs the native observer, which installs, reports and quiesces the motion stage.
target_link_libraries(rsf_anomaly_sr_hooks_test PRIVATE d3dcompiler)
# SR Stage 3 (jitter and mip bias). The jitter test checks the Halton sequence, the row-vector NDC offset, the
# first-person rule, the mip bias delta and the published motion/SR state on the CPU; the Stage 3 swap cases (projection
# and bias restore on every exit path) are cargo tests in games/anomaly/src/sr_swap/tests. The device fixture runs the unchanged core
# motion PS writer with production b13 bytes and jitter (77 when no D3D11 device exists). None of them runs game code.
rsf_add_fixture_test(rsf_anomaly_sr_jitter_test NAME anomaly_sr_jitter_contract ENTRY rsf_anomaly_sr_jitter_fixture ARGV CXX
    LIBS rsf_anomaly_fixtures_rust TIMEOUT 120)
rsf_add_fixture_test(rsf_anomaly_sr_jitter_device_test NAME anomaly_sr_jitter_device ENTRY rsf_anomaly_sr_jitter_device_fixture CXX
    LIBS rsf_anomaly_fixtures_rust d3d11 dxgi SKIP77 TIMEOUT 120)
target_include_directories(rsf_anomaly_sr_jitter_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
target_include_directories(rsf_anomaly_sr_jitter_device_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
# The Stage 3 burst measurement (tools/anomaly_sr_evidence.py --measure-jitter) on a synthetic shifted burst; 77 without numpy.
if(Python3_Interpreter_FOUND)
    add_test(NAME anomaly_sr_measure_self_test COMMAND "${Python3_EXECUTABLE}" -I "${PROJECT_SOURCE_DIR}/tools/anomaly_sr_evidence.py"
        --measure-self-test)
    set_tests_properties(anomaly_sr_measure_self_test PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)
endif()
# SR Stages 4 and 5 (section 12). The Rust pass tests cover the CPU path. The Runtime API contract is a
# client that imports ReScaleFrame.Runtime.dll like a carrier-loaded adapter and checks rsf_get_d3d11_sr_api plus the
# adapter's table validation. The device fixture runs the production decode/resolve parameters and the prepare and
# reinsert passes on any D3D11 device; its provider case needs the vendor runtimes in the package layout
# (<root>/{streamline,fidelityfx,xess}) and an NVIDIA adapter, and is registered when RSF_ANOMALY_SR_VENDOR_ROOT exists.
add_executable(rsf_anomaly_sr_runtime_api_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_runtime_api_bridge.c")
set_target_properties(rsf_anomaly_sr_runtime_api_test PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_anomaly_sr_runtime_api_test PRIVATE rsf_anomaly_fixtures_rust "$<TARGET_LINKER_FILE:rsf_orchestrator>")
add_dependencies(rsf_anomaly_sr_runtime_api_test rsf_orchestrator)
add_executable(rsf_anomaly_sr_temporal_device_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_sr_temporal_device.c")
# The fixture calls the Runtime's motion passes directly, which only the Runtime archive
# defines; that archive also defines the SR API, so this test links it instead of the DLL.
# One archive carries the fixture and the Runtime (tests/game-sdk, anomaly-sr-device-fixture).
target_link_libraries(rsf_anomaly_sr_temporal_device_test PRIVATE rsf_game_sdk_anomaly_sr_rust
    ReScaleFrame::Graphics d3d11 dxgi)
foreach(rsf_sr5_client rsf_anomaly_sr_runtime_api_test rsf_anomaly_sr_temporal_device_test)
    target_include_directories(${rsf_sr5_client} PRIVATE "${RSF_ANOMALY_SR_SRC}"
        "${PROJECT_SOURCE_DIR}/runtime/backends/dlss/include"
        "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
    target_link_libraries(${rsf_sr5_client} PRIVATE ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_build_options)
endforeach()
add_test(NAME anomaly_sr_runtime_api_contract COMMAND rsf_anomaly_sr_runtime_api_test)
add_test(NAME anomaly_sr_temporal_device COMMAND rsf_anomaly_sr_temporal_device_test)
set_tests_properties(anomaly_sr_temporal_device PROPERTIES SKIP_RETURN_CODE 77)
# eng/fetch-vendor.ps1 writes that layout to vendor/runtimes from the pinned SDKs.
set(rsf_anomaly_sr_vendor_default "${PROJECT_SOURCE_DIR}/vendor/runtimes")
set(RSF_ANOMALY_SR_VENDOR_ROOT "${rsf_anomaly_sr_vendor_default}" CACHE PATH
    "Package layout with streamline/, fidelityfx/ and xess/ for the Anomaly SR provider fixture")
if(RSF_ANOMALY_SR_VENDOR_ROOT AND EXISTS "${RSF_ANOMALY_SR_VENDOR_ROOT}/streamline/sl.interposer.dll")
    file(TO_NATIVE_PATH "${RSF_ANOMALY_SR_VENDOR_ROOT}" rsf_anomaly_sr_vendor_native)
    add_test(NAME anomaly_sr_provider_device COMMAND rsf_anomaly_sr_temporal_device_test provider "${rsf_anomaly_sr_vendor_native}")
    set_tests_properties(anomaly_sr_provider_device PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 300)
endif()
# Backend switching through the shared pipeline on hardware (generic runtime behaviour, driven by the same vendor root).
# The fixture is in the Runtime archive and loads the Runtime DLL it switches; it links no import library.
rsf_add_fixture_test(rsf_sr_backend_switch_device_test ENTRY rsf_sr_backend_switch_fixture ARGV
    LIBS rsf_orchestrator_rust ReScaleFrame::Graphics)
add_dependencies(rsf_sr_backend_switch_device_test rsf_orchestrator)
if(RSF_ANOMALY_SR_VENDOR_ROOT AND EXISTS "${RSF_ANOMALY_SR_VENDOR_ROOT}/streamline/sl.interposer.dll")
    foreach(rsf_switch_mode baseline select select+exposure)
        add_test(NAME sr_backend_switch_${rsf_switch_mode}
            COMMAND rsf_sr_backend_switch_device_test "${rsf_anomaly_sr_vendor_native}" ${rsf_switch_mode})
        set_tests_properties(sr_backend_switch_${rsf_switch_mode} PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 600)
    endforeach()
endif()
if(RSF_ANOMALY_SR_VENDOR_ROOT AND EXISTS "${RSF_ANOMALY_SR_VENDOR_ROOT}/fidelityfx/amd_fidelityfx_upscaler_dx12.dll")
    file(TO_NATIVE_PATH "${RSF_ANOMALY_SR_VENDOR_ROOT}" rsf_anomaly_sr_vendor_native)
    add_test(NAME sr_backend_switch_fsr3_start
        COMMAND rsf_sr_backend_switch_device_test "${rsf_anomaly_sr_vendor_native}" fsr3-start)
    set_tests_properties(sr_backend_switch_fsr3_start PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 300)
endif()
set_tests_properties(anomaly_sr_runtime_api_contract
    anomaly_sr_temporal_device PROPERTIES TIMEOUT 120)

# Profile-dependent fixtures once more against the AVX executable's tables (native_profile.rs selects them by name and
# SHA-256 in the game; here the argument selects them). anomaly_sr_profile_contract checks both tables.
add_test(NAME anomaly_geometry_hooks_avx_contract COMMAND rsf_anomaly_geometry_hooks_test avx)
add_test(NAME anomaly_sr_hooks_avx_contract COMMAND rsf_anomaly_sr_hooks_test avx)
set_tests_properties(anomaly_geometry_hooks_avx_contract anomaly_sr_hooks_avx_contract PROPERTIES TIMEOUT 120)


add_executable(rsf_anomaly_input_hooks_test "${CMAKE_CURRENT_LIST_DIR}/anomaly_input_hooks.c")
set_target_properties(rsf_anomaly_input_hooks_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON)
target_include_directories(rsf_anomaly_input_hooks_test PRIVATE "${RSF_ANOMALY_SR_SRC}")
target_link_libraries(rsf_anomaly_input_hooks_test PRIVATE rsf_anomaly_fixtures_rust ReScaleFrame::GameSDK rsf_build_options)
add_test(NAME anomaly_input_hooks_standard_contract COMMAND rsf_anomaly_input_hooks_test standard)
add_test(NAME anomaly_input_hooks_avx_contract COMMAND rsf_anomaly_input_hooks_test avx)
rsf_add_fixture_test(rsf_latency_event_contract_test NAME latency_input_event_contract ENTRY rsf_latency_events_fixture
    LIBS rsf_game_sdk_cpu_rust rsf_native_cpu)
