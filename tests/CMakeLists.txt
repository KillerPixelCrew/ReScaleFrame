add_library(rsf_sdk_c_header OBJECT sdk_c_header.c)
rsf_rust_fixture_externs(RSF_FIXTURE_EXTERNS)

# One archive per fixture crate, built with every one of its fixture features. The features only
# add optional dependencies and windows-sys flags, and each executable names its entry symbol at
# compile time, so the linker pulls in only what an executable references.
rsf_add_rust_static(rsf_presentation_fixtures_rust rsf_rust_presentation_fixtures
    rsf-presentation-fixtures rsf_presentation_fixtures
    --features d3d11-host-device-fixtures,gl-device-fixture)
rsf_add_rust_static(rsf_game_sdk_fixtures_rust rsf_rust_game_sdk_fixtures
    rsf-game-sdk-fixtures rsf_game_sdk_fixtures
    --features render-host-fixtures,cpu-fixtures,unity-sr-d3d12-fixture,plugin-session-fixture,fg-cpu-fixture,d3d11-host-fixture,vk-latency-fixtures,xell-fixture)
rsf_add_rust_static(rsf_vulkan_fg_fixtures_rust rsf_rust_vulkan_fg_fixtures
    rsf-vulkan-fg-fixtures rsf_vulkan_fg_fixtures
    --features dispatch,ffx,wsi-dispatch,fsr4-state,dlss,streamline-selected,streamline-dispatch,xess-sr,xess-input-range,xess-device)
rsf_add_rust_static(rsf_capture_fixtures_rust rsf_rust_capture_fixtures
    rsf-capture-fixtures rsf_capture_fixtures
    --features metadata,d3d12,d3d11,vulkan,vulkan-cpu,texture)
# rsf-ac7 with the engine-address overrides the WARP fixtures need. It replaces the shipping
# rsf_ac7_rust archive in those executables, after rsf_ac7_render_scope, which does not link either.
rsf_add_rust_static(rsf_ac7_fixture_rust rsf_rust_ac7_fixture rsf-ac7 rsf_ac7 --features warp-fixture)
target_link_libraries(rsf_ac7_fixture_rust INTERFACE rsf_shader_compiler rsf_ac7_memory)
rsf_add_rust_static(rsf_anomaly_fixtures_rust rsf_rust_anomaly_fixtures
    rsf-anomaly rsf_anomaly
    --features fg-fixture,input-hook-fixture,overlay-device-fixture,motion-hooks-fixture,motion-binder-fixture,motion-device-fixture,native-hook-fixture,sr-fixtures)
include("${CMAKE_CURRENT_SOURCE_DIR}/d3d11_host_rendered_fg_device.cmake")
# Explicit regression investigation only: requires a scratch vendor-SDK root and NVIDIA GPU.
add_executable(rsf_d3d11_host_shared_sr_device EXCLUDE_FROM_ALL d3d11_host_shared_sr_device.c)
set_target_properties(rsf_d3d11_host_shared_sr_device PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_d3d11_host_shared_sr_device PRIVATE
    rsf_presentation_fixtures_rust rsf_orchestrator rsf_build_options d3d11 dxgi dxguid)
find_program(RSF_TEST_RUSTC rustc REQUIRED)
if(MSVC)
    set(rsf_test_rust_target x86_64-pc-windows-msvc)
else()
    set(rsf_test_rust_target x86_64-pc-windows-gnu)
endif()
set(rsf_vulkan_exports_dir "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>")
set(rsf_vulkan_exports_exe "${rsf_vulkan_exports_dir}/rsf_native_vulkan_exports_test.exe")
set(rsf_vulkan_exports_source "${CMAKE_CURRENT_SOURCE_DIR}/native_vulkan_exports.rs")
add_custom_command(OUTPUT "${rsf_vulkan_exports_exe}"
    COMMAND "${CMAKE_COMMAND}" -E make_directory "${rsf_vulkan_exports_dir}"
    COMMAND "${RSF_TEST_RUSTC}" --edition=2024 --target "${rsf_test_rust_target}"
        -D warnings "${rsf_vulkan_exports_source}" -o "${rsf_vulkan_exports_exe}"
    DEPENDS "${rsf_vulkan_exports_source}"
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
    VERBATIM)
add_custom_target(rsf_native_vulkan_exports_test ALL DEPENDS "${rsf_vulkan_exports_exe}")
add_dependencies(rsf_native_vulkan_exports_test rsf_orchestrator)
add_test(NAME native_vulkan_dll_exports COMMAND ${CMAKE_CROSSCOMPILING_EMULATOR}
    "${rsf_vulkan_exports_exe}" "$<TARGET_FILE:rsf_orchestrator>")
# Corpus parity run by hand, documented in runtime/shader_patch/README.md; the device test below is the CTest one.
add_executable(rsf_shader_patch_test EXCLUDE_FROM_ALL fixture_main.c)
target_compile_definitions(rsf_shader_patch_test PRIVATE RSF_FIXTURE_ENTRY=rsf_shader_patch_corpus_main)
target_link_libraries(rsf_shader_patch_test PRIVATE rsf_shader_patch rsf_build_options)
add_executable(rsf_shader_patch_device_test fixture_main.c)
target_compile_definitions(rsf_shader_patch_device_test PRIVATE RSF_FIXTURE_ENTRY=rsf_shader_patch_device_main)
target_link_libraries(rsf_shader_patch_device_test PRIVATE rsf_shader_patch rsf_build_options)
add_test(NAME shader_patch_synthetic_device COMMAND rsf_shader_patch_device_test)
include("${CMAKE_CURRENT_SOURCE_DIR}/d3d11_present_api.cmake")
add_executable(rsf_native_window_test fixture_main.c)
target_compile_definitions(rsf_native_window_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_native_window)
target_link_libraries(rsf_native_window_test PRIVATE rsf_game_sdk_fixtures_rust "$<TARGET_LINKER_FILE:rsf_orchestrator>" rsf_build_options)
add_dependencies(rsf_native_window_test rsf_orchestrator)
add_test(NAME native_window_contract COMMAND rsf_native_window_test)
rsf_add_rust_static(rsf_generation_fixtures_rust rsf_rust_generation_fixtures
    rsf-generation-fixtures rsf_generation_fixtures)
include("${CMAKE_CURRENT_SOURCE_DIR}/d3d11_fg_api.cmake")
include("${CMAKE_CURRENT_SOURCE_DIR}/overlay_dll_api.cmake")
include("${CMAKE_CURRENT_SOURCE_DIR}/anomaly_support.cmake")
include("${CMAKE_CURRENT_SOURCE_DIR}/anomaly_fg.cmake")
include("${CMAKE_CURRENT_SOURCE_DIR}/thin_loader.cmake")
add_executable(rsf_anomaly_plugin_test game-sdk/anomaly_main.c)
target_link_libraries(rsf_anomaly_plugin_test PRIVATE rsf_game_sdk_fixtures_rust ReScaleFrame::GameSDK rsf_build_options)
add_test(NAME anomaly_plugin_contract COMMAND rsf_anomaly_plugin_test
    "$<TARGET_FILE:rsf_game_anomaly>")
add_dependencies(rsf_anomaly_plugin_test rsf_game_anomaly)
add_executable(rsf_anomaly_lifecycle_test game-sdk/anomaly_main.c)
target_compile_definitions(rsf_anomaly_lifecycle_test PRIVATE RSF_ANOMALY_LIFECYCLE_FIXTURE=1)
target_link_libraries(rsf_anomaly_lifecycle_test PRIVATE rsf_game_sdk_fixtures_rust ReScaleFrame::GameSDK rsf_build_options)
add_dependencies(rsf_anomaly_lifecycle_test rsf_game_anomaly)
add_test(NAME anomaly_lifecycle_contract COMMAND rsf_anomaly_lifecycle_test "$<TARGET_FILE:rsf_game_anomaly>")
add_executable(rsf_anomaly_native_hooks_test anomaly_native_hook_leaves.c ../games/anomaly/src/native_hooks_platform.c)
target_include_directories(rsf_anomaly_native_hooks_test PRIVATE ../games/anomaly/src)
target_link_libraries(rsf_anomaly_native_hooks_test PRIVATE rsf_anomaly_fixtures_rust minhook rsf_build_options)
add_test(NAME anomaly_native_hooks_contract COMMAND rsf_anomaly_native_hooks_test)
set_tests_properties(anomaly_native_hooks_contract PROPERTIES TIMEOUT 30)
set(rsf_anomaly_bootstrap_exe "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>/rsf_anomaly_bootstrap_test.exe")
set(rsf_anomaly_bootstrap_source "${CMAKE_CURRENT_SOURCE_DIR}/anomaly_bootstrap.rs")
add_custom_command(OUTPUT "${rsf_anomaly_bootstrap_exe}"
    COMMAND "${CMAKE_COMMAND}" -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>"
    COMMAND "${RSF_TEST_RUSTC}" --edition=2024 --target "${rsf_test_rust_target}"
        -D warnings "${rsf_anomaly_bootstrap_source}" -o "${rsf_anomaly_bootstrap_exe}"
    DEPENDS "${rsf_anomaly_bootstrap_source}"
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
    VERBATIM)
add_custom_target(rsf_anomaly_bootstrap_test ALL DEPENDS "${rsf_anomaly_bootstrap_exe}")
add_dependencies(rsf_anomaly_bootstrap_test rsf_game_anomaly)
add_test(NAME anomaly_bootstrap_contract COMMAND ${CMAKE_CROSSCOMPILING_EMULATOR}
    "${rsf_anomaly_bootstrap_exe}"
    "$<TARGET_FILE:rsf_game_anomaly>" "${CMAKE_CURRENT_BINARY_DIR}/anomaly-bootstrap")
set_tests_properties(anomaly_bootstrap_contract PROPERTIES TIMEOUT 130)
find_program(RSF_PWSH_EXECUTABLE pwsh)
if(RSF_PWSH_EXECUTABLE)
    add_test(NAME ffx_line_width_patch COMMAND "${RSF_PWSH_EXECUTABLE}" -NoProfile -File "${CMAKE_CURRENT_SOURCE_DIR}/ffx_line_width_patch.ps1" -SdkRoot "${RSF_FFX_VK_INCLUDE}/../..")
    set_tests_properties(ffx_line_width_patch PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 30)
endif()
target_link_libraries(rsf_sdk_c_header PRIVATE ReScaleFrame::GameSDK rsf_build_options)

add_executable(rsf_plugin_contract game-sdk/contract_main.c)

target_link_libraries(rsf_plugin_contract PRIVATE rsf_game_sdk_fixtures_rust ReScaleFrame::GameSDK rsf_build_options)
add_dependencies(rsf_plugin_contract rsf_game_ac7 rsf_game_project_wingman rsf_game_rdr2 rsf_game_unity_mono)
add_dependencies(rsf_plugin_contract rsf_game_skyrim_se)
add_dependencies(rsf_plugin_contract rsf_game_fallout4)
add_test(NAME game_fallout4_plugin_contract COMMAND rsf_plugin_contract
    "$<TARGET_FILE_NAME:rsf_game_fallout4>" fallout4)
set_tests_properties(game_fallout4_plugin_contract PROPERTIES
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_game_fallout4>")
add_executable(rsf_unity_sr_d3d12_test unity_sr_d3d12_preset.c)
set_target_properties(rsf_unity_sr_d3d12_test PROPERTIES LINKER_LANGUAGE CXX)
# The toolchain CRT calls the C ABI main from the fixture archive.
if(MSVC)
    target_link_options(rsf_unity_sr_d3d12_test PRIVATE "LINKER:/SUBSYSTEM:CONSOLE" "LINKER:/INCLUDE:main")
elseif(MINGW)
    target_link_options(rsf_unity_sr_d3d12_test PRIVATE "LINKER:--undefined=main")
endif()
target_include_directories(rsf_unity_sr_d3d12_test PRIVATE ../games/unity-mono/include)
target_link_libraries(rsf_unity_sr_d3d12_test PRIVATE rsf_game_sdk_fixtures_rust rsf_rdr2_rust rsf_orchestrator rsf_build_options minhook d3d12 dxgi)
add_test(NAME unity_sr_d3d12_contract COMMAND rsf_unity_sr_d3d12_test)
add_executable(rsf_render_frontend_test fixture_main.c)
target_compile_definitions(rsf_render_frontend_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_render_frontend)
target_link_libraries(rsf_render_frontend_test PRIVATE rsf_game_sdk_fixtures_rust rsf_orchestrator rsf_build_options)
add_test(NAME render_frontend_contract COMMAND rsf_render_frontend_test)
add_executable(rsf_render_frontend_embedded_test fixture_main.c)
target_compile_definitions(rsf_render_frontend_embedded_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_render_frontend)
target_link_libraries(rsf_render_frontend_embedded_test PRIVATE rsf_game_sdk_fixtures_rust ReScaleFrame::OrchestratorCore rsf_build_options)
add_test(NAME render_frontend_embedded COMMAND rsf_render_frontend_embedded_test)
add_executable(rsf_callback_host_abi_test fixture_main.c)
target_compile_definitions(rsf_callback_host_abi_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_callback_host_abi)
target_link_libraries(rsf_callback_host_abi_test PRIVATE rsf_game_sdk_fixtures_rust rsf_orchestrator rsf_build_options)
add_test(NAME callback_host_abi COMMAND rsf_callback_host_abi_test)
add_executable(rsf_callback_host_abi_embedded_test fixture_main.c)
target_compile_definitions(rsf_callback_host_abi_embedded_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_callback_host_abi)
target_link_libraries(rsf_callback_host_abi_embedded_test PRIVATE rsf_game_sdk_fixtures_rust ReScaleFrame::OrchestratorCore rsf_build_options)
add_test(NAME callback_host_abi_embedded COMMAND rsf_callback_host_abi_embedded_test)
set_tests_properties(callback_host_abi callback_host_abi_embedded PROPERTIES TIMEOUT 30)
add_executable(rsf_orchestrator_domains fixture_main.c)
target_compile_definitions(rsf_orchestrator_domains PRIVATE RSF_FIXTURE_ENTRY=rsf_test_orchestrator_domains)
target_link_libraries(rsf_orchestrator_domains PRIVATE rsf_game_sdk_fixtures_rust rsf_build_options)
add_dependencies(rsf_orchestrator_domains rsf_orchestrator rsf_game_unity_mono)
add_test(NAME orchestrator_domain_contract COMMAND rsf_orchestrator_domains)
add_executable(rsf_d3d12_observer_test d3d12_observer.c)
set_target_properties(rsf_d3d12_observer_test PROPERTIES LINKER_LANGUAGE CXX)
# The D3D12 observer is implemented in the Runtime archive.
target_link_libraries(rsf_d3d12_observer_test PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options d3d12)
add_test(NAME d3d12_observer_contract COMMAND rsf_d3d12_observer_test)
add_executable(rsf_exposure_normalization_test exposure_normalization_main.c)
# The fixture is in the Runtime archive, whose member also defines the SR owner entries it is
# handed; the DLL would define them a second time, so the test links the archive alone.
target_link_libraries(rsf_exposure_normalization_test PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics
    rsf_build_options d3d12 dxgi)
add_test(NAME native_exposure_normalization COMMAND rsf_exposure_normalization_test)
add_executable(rsf_gpu_policy_test fixture_main.c)
target_compile_definitions(rsf_gpu_policy_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_gpu_policy)
target_link_libraries(rsf_gpu_policy_test PRIVATE rsf_generation_fixtures_rust rsf_orchestrator rsf_build_options)
add_test(NAME gpu_sr_policy COMMAND rsf_gpu_policy_test)
add_executable(rsf_gpu_policy_embedded_test fixture_main.c)
target_compile_definitions(rsf_gpu_policy_embedded_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_gpu_policy)
target_link_libraries(rsf_gpu_policy_embedded_test PRIVATE rsf_generation_fixtures_rust ReScaleFrame::OrchestratorCore rsf_build_options)
add_test(NAME gpu_sr_policy_embedded COMMAND rsf_gpu_policy_embedded_test)
add_library(rsf_plugin_session_fixture SHARED plugin_session_fixture_throw.c)
target_link_libraries(rsf_plugin_session_fixture PRIVATE rsf_game_sdk_fixtures_rust ReScaleFrame::GameSDK rsf_build_options)
add_executable(rsf_plugin_session_test game-sdk/session_main.c)
target_link_libraries(rsf_plugin_session_test PRIVATE rsf_game_sdk_fixtures_rust rsf_orchestrator rsf_build_options)
add_dependencies(rsf_plugin_session_test rsf_plugin_session_fixture)
add_test(NAME plugin_session_lifetime COMMAND rsf_plugin_session_test $<TARGET_FILE:rsf_plugin_session_fixture>)
add_executable(rsf_plugin_session_embedded_test game-sdk/session_main.c)
target_link_libraries(rsf_plugin_session_embedded_test PRIVATE rsf_game_sdk_fixtures_rust ReScaleFrame::OrchestratorCore rsf_build_options)
add_dependencies(rsf_plugin_session_embedded_test rsf_plugin_session_fixture)
add_test(NAME plugin_session_lifetime_embedded COMMAND rsf_plugin_session_embedded_test $<TARGET_FILE:rsf_plugin_session_fixture>)
set_tests_properties(plugin_session_lifetime plugin_session_lifetime_embedded PROPERTIES TIMEOUT 30)
find_program(RSF_RUSTC rustc REQUIRED)
if(MSVC)
    set(unity_mono_fixture_target x86_64-pc-windows-msvc)
else()
    set(unity_mono_fixture_target x86_64-pc-windows-gnu)
endif()
set(unity_mono_fixture_dir "${CMAKE_RUNTIME_OUTPUT_DIRECTORY}/$<CONFIG>")
set(unity_mono_fixture_exe "${unity_mono_fixture_dir}/rsf_unity_mono_runtime_test.exe")
add_custom_command(OUTPUT "${unity_mono_fixture_exe}"
    COMMAND "${CMAKE_COMMAND}" -E make_directory "${unity_mono_fixture_dir}"
    COMMAND "${RSF_RUSTC}" --edition=2024 --target ${unity_mono_fixture_target}
        -Dwarnings "${CMAKE_CURRENT_SOURCE_DIR}/unity_mono_runtime.rs" -o "${unity_mono_fixture_exe}"
    DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/unity_mono_runtime.rs" "${PROJECT_SOURCE_DIR}/rust-toolchain.toml"
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
    VERBATIM)
add_custom_target(rsf_unity_mono_runtime_test ALL DEPENDS "${unity_mono_fixture_exe}")
if(MSVC)
    rsf_add_loader_test(rsf_plugin_carrier_test rsf_test_xinput_carrier)
    add_dependencies(rsf_plugin_carrier_test rsf_proxy_plugin)
    add_test(NAME plugin_carrier_contract COMMAND rsf_plugin_carrier_test "$<TARGET_FILE:rsf_proxy_plugin>")
    set_tests_properties(plugin_carrier_contract PROPERTIES SKIP_RETURN_CODE 77)
    rsf_add_loader_test(rsf_shared_shim_exports rsf_test_shared_shim_exports)
    add_dependencies(rsf_shared_shim_exports rsf_proxy_dinput8)
    add_test(NAME shared_shim_exports COMMAND rsf_shared_shim_exports "$<TARGET_FILE:rsf_proxy_dinput8>")
endif()
# Load the plugin by name from its own directory. That is how it is loaded in practice, and it
# keeps the test runnable under a cross-build emulator, which cannot take a host path.
add_test(NAME game_plugin_contract COMMAND rsf_plugin_contract "$<TARGET_FILE_NAME:rsf_game_ac7>" ac7)
add_test(NAME game_skyrim_se_plugin_contract COMMAND rsf_plugin_contract
    "$<TARGET_FILE_NAME:rsf_game_skyrim_se>" skyrim-se)
set_tests_properties(game_skyrim_se_plugin_contract PROPERTIES
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_game_skyrim_se>")
set_tests_properties(game_plugin_contract PROPERTIES
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_game_ac7>")
add_test(NAME game_project_wingman_plugin_contract COMMAND rsf_plugin_contract
    "$<TARGET_FILE_NAME:rsf_game_project_wingman>" project-wingman)
set_tests_properties(game_project_wingman_plugin_contract PROPERTIES
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_game_project_wingman>")
add_test(NAME game_rdr2_plugin_contract COMMAND rsf_plugin_contract
    "$<TARGET_FILE_NAME:rsf_game_rdr2>" rdr2)
set_tests_properties(game_rdr2_plugin_contract PROPERTIES
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_game_rdr2>")
add_test(NAME game_unity_mono_plugin_contract COMMAND rsf_plugin_contract
    "$<TARGET_FILE_NAME:rsf_game_unity_mono>" unity-mono)
set_tests_properties(game_unity_mono_plugin_contract PROPERTIES
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_game_unity_mono>")
add_executable(rsf_module_dump_entropy module_dump_entropy.c)
target_link_libraries(rsf_module_dump_entropy PRIVATE ReScaleFrame::LoaderDiagnostics rsf_build_options)
add_test(NAME module_dump_entropy COMMAND rsf_module_dump_entropy)

add_executable(rsf_module_dump_self module_dump_self.c)
target_link_libraries(rsf_module_dump_self PRIVATE ReScaleFrame::LoaderDiagnostics rsf_build_options)
target_compile_definitions(rsf_module_dump_self PRIVATE _CRT_SECURE_NO_WARNINGS)
add_test(NAME module_dump_self COMMAND rsf_module_dump_self "${CMAKE_CURRENT_BINARY_DIR}")

function(rsf_add_capture_fixture target entry source)
    add_executable(${target} ${source})
    target_compile_definitions(${target} PRIVATE RSF_CAPTURE_FIXTURE=${entry})
    target_link_libraries(${target} PRIVATE rsf_capture_fixtures_rust rsf_build_options)
endfunction()

rsf_add_capture_fixture(rsf_texture_dump rsf_fixture_texture_dump capture_fixture_main.c)
target_link_libraries(rsf_texture_dump PRIVATE ReScaleFrame::GraphicsLeaves ReScaleFrame::Graphics rsf_build_options)
add_test(NAME texture_dump COMMAND rsf_texture_dump "${CMAKE_CURRENT_BINARY_DIR}")

rsf_add_capture_fixture(rsf_input_dump_test rsf_fixture_input_dump_d3d11 capture_fixture_main.c)
add_library(rsf_input_dump_c_header OBJECT input_dump_c_header.c)
target_link_libraries(rsf_input_dump_c_header PRIVATE ReScaleFrame::FrameDump rsf_build_options)
target_link_libraries(rsf_input_dump_test PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options windowscodecs ole32)
add_test(NAME input_dump COMMAND rsf_input_dump_test "${CMAKE_CURRENT_BINARY_DIR}")
set_tests_properties(input_dump PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

rsf_add_capture_fixture(rsf_input_dump_d3d12 rsf_fixture_input_dump_d3d12 capture_fixture_main.c)
target_compile_definitions(rsf_input_dump_d3d12 PRIVATE RSF_FRAME_DUMP_STATIC)
target_link_libraries(rsf_input_dump_d3d12 PRIVATE ReScaleFrame::FrameDump rsf_build_options d3d12 dxgi)
add_test(NAME input_dump_d3d12 COMMAND rsf_input_dump_d3d12 "${CMAKE_CURRENT_BINARY_DIR}")
set_tests_properties(input_dump_d3d12 PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

if(RSF_FRAME_DUMP_VULKAN_INCLUDE)
    rsf_add_capture_fixture(rsf_input_dump_vulkan_cpu rsf_fixture_input_dump_vulkan_cpu
        capture_vulkan_fixture_main.c)
    target_link_libraries(rsf_input_dump_vulkan_cpu PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME input_dump_vulkan_cpu_linked COMMAND rsf_input_dump_vulkan_cpu "${CMAKE_CURRENT_BINARY_DIR}")

    add_executable(rsf_input_dump_vulkan_cpu_dll capture_vulkan_fixture_main.c)
    target_compile_definitions(rsf_input_dump_vulkan_cpu_dll PRIVATE RSF_CAPTURE_FIXTURE=rsf_fixture_input_dump_vulkan_cpu)
    target_link_libraries(rsf_input_dump_vulkan_cpu_dll PRIVATE rsf_capture_fixtures_rust)
    target_include_directories(rsf_input_dump_vulkan_cpu_dll PRIVATE
        "$<TARGET_PROPERTY:rsf_frame_dump,INTERFACE_INCLUDE_DIRECTORIES>")
    target_include_directories(rsf_input_dump_vulkan_cpu_dll SYSTEM PRIVATE "${RSF_FRAME_DUMP_VULKAN_INCLUDE}")
    target_link_libraries(rsf_input_dump_vulkan_cpu_dll PRIVATE "$<TARGET_LINKER_FILE:rsf_orchestrator>"
        ReScaleFrame::GameSDK ReScaleFrame::Graphics ReScaleFrame::Contract rsf_build_options)
    add_dependencies(rsf_input_dump_vulkan_cpu_dll rsf_orchestrator)
    add_test(NAME input_dump_vulkan_cpu_dll COMMAND rsf_input_dump_vulkan_cpu_dll "${CMAKE_CURRENT_BINARY_DIR}")

    rsf_add_capture_fixture(rsf_input_dump_vulkan rsf_fixture_input_dump_vulkan
        capture_vulkan_fixture_main.c)
    target_include_directories(rsf_input_dump_vulkan SYSTEM PRIVATE "${RSF_FRAME_DUMP_VULKAN_INCLUDE}")
    target_compile_definitions(rsf_input_dump_vulkan PRIVATE RSF_RUNTIME_BUILD)
    target_link_libraries(rsf_input_dump_vulkan PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME input_dump_vulkan COMMAND rsf_input_dump_vulkan "${CMAKE_CURRENT_BINARY_DIR}")
    set_tests_properties(input_dump_vulkan PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)
endif()

add_executable(rsf_frame_assembly fixture_main.c)
target_compile_definitions(rsf_frame_assembly PRIVATE RSF_FIXTURE_ENTRY=rsf_test_frame_assembly)
target_link_libraries(rsf_frame_assembly PRIVATE rsf_generation_fixtures_rust rsf_orchestrator rsf_build_options)
add_test(NAME frame_assembly COMMAND rsf_frame_assembly)

add_executable(rsf_ac7_velocity_shader_test ac7_velocity_shader.c)
set_target_properties(rsf_ac7_velocity_shader_test PROPERTIES LINKER_LANGUAGE CXX)
if(MINGW)
    target_link_options(rsf_ac7_velocity_shader_test PRIVATE -municode)
endif()
target_link_libraries(rsf_ac7_velocity_shader_test PRIVATE ReScaleFrame::AC7RenderScope rsf_ac7_fixture_rust ReScaleFrame::Graphics ReScaleFrame::RuntimeCommon
    ReScaleFrame::GraphicsLeaves rsf_build_options d3d11)
add_test(NAME ac7_velocity_shader COMMAND rsf_ac7_velocity_shader_test)
set_tests_properties(ac7_velocity_shader PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

rsf_add_rust_static(rsf_ac7_capture_fixture_rust rsf_rust_ac7_capture_fixture
    rsf-ac7-capture-fixture rsf_ac7_capture_fixture)
add_executable(rsf_ac7_capture_io fixture_main.c)
target_compile_definitions(rsf_ac7_capture_io PRIVATE RSF_FIXTURE_ENTRY=rsf_test_ac7_capture_io)
# The fixture installs the Runtime's D3D11 observer, so the Runtime archive also supplies the leaves.
target_link_libraries(rsf_ac7_capture_io PRIVATE rsf_ac7_capture_fixture_rust
    ReScaleFrame::AC7MotionCapture rsf_orchestrator_rust rsf_build_options d3dcompiler)
add_test(NAME ac7_capture_io COMMAND rsf_ac7_capture_io)
set_tests_properties(ac7_capture_io PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

if(DEFINED ENV{RUSTC} AND NOT "$ENV{RUSTC}" STREQUAL "")
    set(ac7_fixture_rustc "$ENV{RUSTC}")
else()
    find_program(ac7_fixture_rustc rustc REQUIRED)
endif()
if(MSVC)
    set(ac7_fixture_target x86_64-pc-windows-msvc)
else()
    set(ac7_fixture_target x86_64-pc-windows-gnu)
endif()
function(rsf_ac7_fixture_object output stem)
    set(object "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>/rsf_${stem}.obj")
    add_custom_command(OUTPUT "${object}"
        COMMAND "${CMAKE_COMMAND}" -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>"
        COMMAND "${ac7_fixture_rustc}" --edition 2024 --target "${ac7_fixture_target}"
            --crate-name "rsf_${stem}" --crate-type lib --emit obj -C codegen-units=1
            -C panic=abort -C opt-level=$<IF:$<CONFIG:Debug>,0,3> ${RSF_FIXTURE_EXTERNS}
            -o "${object}" "${CMAKE_CURRENT_SOURCE_DIR}/${stem}.rs"
        DEPENDS "${stem}.rs" rsf_rust_fixture_rlibs ${ARGN}
        WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
        VERBATIM)
    set_source_files_properties("${object}" PROPERTIES GENERATED TRUE EXTERNAL_OBJECT TRUE)
    set(${output} "${object}" PARENT_SCOPE)
endfunction()

rsf_ac7_fixture_object(ac7_view_probe_object ac7_view_probe)
add_executable(rsf_ac7_view_test ac7_view.c "${ac7_view_probe_object}")
set_target_properties(rsf_ac7_view_test PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_ac7_view_test PRIVATE ReScaleFrame::AC7View ReScaleFrame::GraphicsLeaves rsf_build_options)
add_test(NAME ac7_view COMMAND rsf_ac7_view_test)

set(ac7_session_object "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>/rsf_ac7_session.obj")
add_custom_command(OUTPUT "${ac7_session_object}"
    COMMAND "${CMAKE_COMMAND}" -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>"
    COMMAND "${ac7_fixture_rustc}" --edition 2024 --target "${ac7_fixture_target}"
        --crate-name rsf_ac7_session --crate-type lib --emit obj -C codegen-units=1
        -C panic=abort -C opt-level=$<IF:$<CONFIG:Debug>,0,3> ${RSF_FIXTURE_EXTERNS}
        -o "${ac7_session_object}" "${CMAKE_CURRENT_SOURCE_DIR}/ac7_session.rs"
    DEPENDS ac7_session.rs rsf_rust_fixture_rlibs
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
    VERBATIM)
set_source_files_properties("${ac7_session_object}" PROPERTIES GENERATED TRUE EXTERNAL_OBJECT TRUE)
add_executable(rsf_ac7_session fixture_main.c "${ac7_session_object}")
target_compile_definitions(rsf_ac7_session PRIVATE RSF_FIXTURE_ENTRY=rsf_ac7_session_fixture)
set_target_properties(rsf_ac7_session PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_ac7_session PRIVATE ReScaleFrame::AC7SceneColor ReScaleFrame::GraphicsLeaves rsf_build_options)
add_test(NAME ac7_session COMMAND rsf_ac7_session)

include(g02/fixture.cmake)
rsf_add_g02_fixture(rsf_constant_override constant_override.rs)
target_link_libraries(rsf_constant_override PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options d3dcompiler)
add_test(NAME constant_override COMMAND rsf_constant_override)
set_tests_properties(constant_override PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

# No device: the classifier is a pure function over shadowed facts, which is the point of it.
add_executable(rsf_ac7_ui_rules_test ac7_ui_rules.c)
set_target_properties(rsf_ac7_ui_rules_test PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_ac7_ui_rules_test PRIVATE ReScaleFrame::AC7UIRules ReScaleFrame::GraphicsLeaves rsf_build_options)
add_test(NAME ac7_ui_rules COMMAND rsf_ac7_ui_rules_test)

# A vendor that is not compiled in still has to answer, and has to answer differently from one
# whose runtime is missing. Runs in both configurations.
add_executable(rsf_fsr_backend fsr_backend.c)
set_target_properties(rsf_fsr_backend PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_fsr_backend PRIVATE ReScaleFrame::FSR ReScaleFrame::XeSS rsf_build_options)
add_test(NAME fsr_backend COMMAND rsf_fsr_backend)

# Whether a D3D11 texture can be opened by a D3D12 device here at all. Skips with 77 and the
# HRESULTs when the two runtimes disagree, which under Wine is a fact about the environment.
add_executable(rsf_shared_surface presentation/shared_surface_main.c)
set_target_properties(rsf_shared_surface PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON
    LINKER_LANGUAGE CXX)
target_link_libraries(rsf_shared_surface PRIVATE rsf_presentation_fixtures_rust
    ReScaleFrame::Presentation rsf_build_options)
add_test(NAME shared_surface COMMAND rsf_shared_surface)
set_tests_properties(shared_surface PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

# The layer, and the alpha accumulation that decides whether diverting into it works at all.
add_executable(rsf_ui_layer fixture_main.c)
target_compile_definitions(rsf_ui_layer PRIVATE RSF_FIXTURE_ENTRY=rsf_ui_layer_fixture)
target_link_libraries(rsf_ui_layer PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options)
add_test(NAME ui_layer COMMAND rsf_ui_layer)
set_tests_properties(ui_layer PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

# The composite's arithmetic, which nothing covered while it lived in present_blit.
add_executable(rsf_fullscreen_pass fixture_main.c)
target_compile_definitions(rsf_fullscreen_pass PRIVATE RSF_FIXTURE_ENTRY=rsf_fullscreen_pass_fixture)
target_link_libraries(rsf_fullscreen_pass PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options)
add_test(NAME fullscreen_pass COMMAND rsf_fullscreen_pass)
set_tests_properties(fullscreen_pass PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_native_reinsert fixture_main.c)
target_compile_definitions(rsf_native_reinsert PRIVATE RSF_FIXTURE_ENTRY=rsf_native_reinsert_fixture)
target_include_directories(rsf_native_reinsert PRIVATE ../runtime/orchestrator/include)
target_compile_definitions(rsf_native_reinsert PRIVATE RSF_RUNTIME_BUILD)
target_link_libraries(rsf_native_reinsert PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics ReScaleFrame::GameSDK rsf_build_options)
add_test(NAME native_reinsert_device COMMAND rsf_native_reinsert)
set_tests_properties(native_reinsert_device PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_depth_scale fixture_main.c)
target_compile_definitions(rsf_depth_scale PRIVATE RSF_FIXTURE_ENTRY=rsf_depth_scale_fixture)
target_link_libraries(rsf_depth_scale PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options)
add_test(NAME depth_scale_device COMMAND rsf_depth_scale)
set_tests_properties(depth_scale_device PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_auxiliary_sr fixture_main.c)
target_compile_definitions(rsf_auxiliary_sr PRIVATE RSF_FIXTURE_ENTRY=rsf_auxiliary_sr_fixture)
target_link_libraries(rsf_auxiliary_sr PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics ReScaleFrame::GameSDK rsf_build_options)
add_test(NAME auxiliary_sr_device COMMAND rsf_auxiliary_sr)
set_tests_properties(auxiliary_sr_device PROPERTIES TIMEOUT 60 SKIP_RETURN_CODE 77)

add_executable(rsf_skyrim_input skyrim_input_main.c ../games/skyrim-se/src/guarded_read.c)
set_target_properties(rsf_skyrim_input PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON LINKER_LANGUAGE CXX)
target_link_libraries(rsf_skyrim_input PRIVATE rsf_skyrim_se_rust ReScaleFrame::GameSDK rsf_steam_native minhook rsf_build_options)
add_test(NAME skyrim_input_contract COMMAND rsf_skyrim_input)

add_executable(rsf_native_cpu_reservation_test fixture_main.c)
target_compile_definitions(rsf_native_cpu_reservation_test PRIVATE RSF_FIXTURE_ENTRY=rsf_cpu_reservation_fixture)
target_link_libraries(rsf_native_cpu_reservation_test PRIVATE rsf_game_sdk_fixtures_rust rsf_native_cpu rsf_build_options)
add_test(NAME native_cpu_reservation_contract COMMAND rsf_native_cpu_reservation_test)

add_executable(rsf_native_fg_cpu_test fixture_main.c)
target_compile_definitions(rsf_native_fg_cpu_test PRIVATE RSF_FIXTURE_ENTRY=rsf_native_fg_cpu_fixture)
target_link_libraries(rsf_native_fg_cpu_test PRIVATE rsf_game_sdk_fixtures_rust ReScaleFrame::OrchestratorCore rsf_build_options)
add_test(NAME native_fg_cpu_contract COMMAND rsf_native_fg_cpu_test)
set_tests_properties(native_fg_cpu_contract PROPERTIES TIMEOUT 15)

add_executable(rsf_sampler_bias fixture_main.c)
target_compile_definitions(rsf_sampler_bias PRIVATE RSF_FIXTURE_ENTRY=rsf_sampler_bias_fixture)
target_include_directories(rsf_sampler_bias PRIVATE ../runtime/orchestrator/include ../runtime/presentation/include)
target_link_libraries(rsf_sampler_bias PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics ReScaleFrame::GameSDK rsf_build_options)
add_test(NAME sampler_bias_device COMMAND rsf_sampler_bias)
set_tests_properties(sampler_bias_device PROPERTIES TIMEOUT 60 SKIP_RETURN_CODE 77)

add_executable(rsf_d3d11_host_test d3d11_host_c_header.c)
set_target_properties(rsf_d3d11_host_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON
    LINKER_LANGUAGE CXX)
target_include_directories(rsf_d3d11_host_test PRIVATE "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
target_link_libraries(rsf_d3d11_host_test PRIVATE rsf_game_sdk_fixtures_rust "$<TARGET_LINKER_FILE:rsf_orchestrator>" rsf_build_options)
add_dependencies(rsf_d3d11_host_test rsf_orchestrator)
add_test(NAME d3d11_host_contract COMMAND rsf_d3d11_host_test)

add_executable(rsf_resource_roles fixture_main.c)
target_compile_definitions(rsf_resource_roles PRIVATE RSF_FIXTURE_ENTRY=rsf_resource_roles_fixture)
target_link_libraries(rsf_resource_roles PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options)
add_test(NAME resource_roles COMMAND rsf_resource_roles)

add_executable(rsf_motion_decode fixture_main.c)
target_compile_definitions(rsf_motion_decode PRIVATE RSF_FIXTURE_ENTRY=rsf_motion_decode_fixture)
target_link_libraries(rsf_motion_decode PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options)
add_test(NAME motion_decode COMMAND rsf_motion_decode)
set_tests_properties(motion_decode PROPERTIES TIMEOUT 120)

add_executable(rsf_frame_tap frame_tap.c)
set_target_properties(rsf_frame_tap PROPERTIES LINKER_LANGUAGE CXX)
# The frame tap's depth replay is Runtime code, so the Runtime archive also supplies the leaves.
target_link_libraries(rsf_frame_tap PRIVATE ReScaleFrame::AC7SceneColor rsf_orchestrator_rust ReScaleFrame::Graphics
    rsf_build_options)
add_test(NAME frame_tap COMMAND rsf_frame_tap)
set_tests_properties(frame_tap PROPERTIES TIMEOUT 120)

add_executable(rsf_d3d11_state fixture_main.c)
target_compile_definitions(rsf_d3d11_state PRIVATE RSF_FIXTURE_ENTRY=rsf_d3d11_state_fixture)
target_link_libraries(rsf_d3d11_state PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options)
add_test(NAME d3d11_state COMMAND rsf_d3d11_state)
set_tests_properties(d3d11_state PROPERTIES TIMEOUT 120)

rsf_add_g02_fixture(rsf_scene_promote scene_promote.rs)
target_link_libraries(rsf_scene_promote PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options)
add_test(NAME scene_promote COMMAND rsf_scene_promote)
set_tests_properties(scene_promote PROPERTIES TIMEOUT 120)

add_executable(rsf_d3d11_observer d3d11_observer.c)
set_target_properties(rsf_d3d11_observer PROPERTIES LINKER_LANGUAGE CXX)
# The observer is implemented in the Runtime archive.
target_link_libraries(rsf_d3d11_observer PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options)
add_test(NAME d3d11_observer COMMAND rsf_d3d11_observer "${CMAKE_CURRENT_BINARY_DIR}")
# These touch a graphics runtime and patch vtables, so a deadlock is a plausible failure. Fail in
# a couple of minutes rather than hanging the suite.
set_tests_properties(d3d11_observer texture_dump PROPERTIES TIMEOUT 120)

find_program(RSF_RUSTC rustc REQUIRED)
if(MSVC)
    set(overlay_fixture_rust_target x86_64-pc-windows-msvc)
else()
    set(overlay_fixture_rust_target x86_64-pc-windows-gnu)
endif()
set(overlay_fixture_dir "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>")
set(overlay_fixture_dll "${overlay_fixture_dir}/rsf_overlay_panel_fixture.dll")
set(overlay_fixture_source "${CMAKE_CURRENT_SOURCE_DIR}/overlay_panel_fixture.rs")
add_custom_command(OUTPUT "${overlay_fixture_dll}"
    COMMAND "${CMAKE_COMMAND}" -E make_directory "${overlay_fixture_dir}"
    COMMAND "${RSF_RUSTC}" --edition=2024 --crate-type=cdylib --target ${overlay_fixture_rust_target}
        -Cpanic=abort -Dwarnings ${RSF_FIXTURE_EXTERNS} "${overlay_fixture_source}"
        -o "${overlay_fixture_dll}"
    DEPENDS "${overlay_fixture_source}" "${PROJECT_SOURCE_DIR}/ui/overlay/src/abi.rs"
        "${PROJECT_SOURCE_DIR}/rust-toolchain.toml" rsf_rust_fixture_rlibs
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
    VERBATIM)
add_custom_target(rsf_overlay_panel_fixture DEPENDS "${overlay_fixture_dll}")
rsf_add_rust_static(rsf_overlay_fixtures_rust rsf_rust_overlay_fixtures
    rsf-overlay-fixtures rsf_overlay_fixtures)
add_executable(rsf_overlay_host fixture_main.c)
target_compile_definitions(rsf_overlay_host PRIVATE RSF_FIXTURE_ENTRY=rsf_test_overlay_host)
target_link_libraries(rsf_overlay_host PRIVATE rsf_overlay_fixtures_rust rsf_orchestrator_rust
    ReScaleFrame::Graphics rsf_build_options)

add_executable(rsf_overlay_hotkey fixture_main.c)
target_compile_definitions(rsf_overlay_hotkey PRIVATE RSF_FIXTURE_ENTRY=rsf_test_overlay_hotkey)
target_link_libraries(rsf_overlay_hotkey PRIVATE rsf_overlay_fixtures_rust rsf_orchestrator_rust
    ReScaleFrame::Graphics rsf_build_options)
add_test(NAME overlay_hotkey COMMAND rsf_overlay_hotkey)
set_tests_properties(overlay_hotkey PROPERTIES TIMEOUT 30)
add_dependencies(rsf_overlay_host rsf_overlay_panel_fixture)
add_test(NAME overlay_host COMMAND rsf_overlay_host "${overlay_fixture_dll}")
set_tests_properties(overlay_host PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_overlay_host>")
add_executable(rsf_overlay_native12_test fixture_main.c overlay_native12.c)
target_compile_definitions(rsf_overlay_native12_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_overlay_native12_run)
target_include_directories(rsf_overlay_native12_test PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_overlay_native12_test PRIVATE rsf_overlay_fixtures_rust rsf_orchestrator_rust
    ReScaleFrame::Graphics rsf_build_options d3d12 d3dcompiler dxgi)
add_dependencies(rsf_overlay_native12_test rsf_overlay_panel_fixture)
add_test(NAME overlay_native12_backpressure COMMAND rsf_overlay_native12_test "${overlay_fixture_dll}")
set_tests_properties(overlay_native12_backpressure PROPERTIES TIMEOUT 30 SKIP_RETURN_CODE 77)

add_executable(rsf_dlss_backend fixture_main.c)
target_compile_definitions(rsf_dlss_backend PRIVATE RSF_FIXTURE_ENTRY=rsf_fixture_dlss_backend)
target_link_libraries(rsf_dlss_backend PRIVATE ReScaleFrame::DLSS rsf_build_options)
add_test(NAME dlss_backend COMMAND rsf_dlss_backend)
# Loading a vendor runtime that talks to a driver is another place a hang is plausible.
set_tests_properties(dlss_backend PROPERTIES TIMEOUT 120)

# Rust builds the Streamline records for DLSS SR. sl.h is C++ only, so a C++ translation unit
# reports the SDK's layouts, defaults and enum values and the Rust fixture compares them.
if(RSF_HAVE_STREAMLINE)
    add_executable(rsf_dlss_sr_layout dlss_sr_layout_main.c dlss_sr_layout_facts.cpp)
    target_include_directories(rsf_dlss_sr_layout SYSTEM PRIVATE "${RSF_STREAMLINE_INCLUDE}")
    target_link_libraries(rsf_dlss_sr_layout PRIVATE ReScaleFrame::DLSS rsf_build_options d3d12)
    add_test(NAME dlss_sr_layout COMMAND rsf_dlss_sr_layout)
endif()

add_executable(rsf_sr_bridge_test fixture_main_args.c)
target_compile_definitions(rsf_sr_bridge_test PRIVATE RSF_FIXTURE_ENTRY=rsf_sr_bridge_fixture)
target_link_libraries(rsf_sr_bridge_test PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options)
add_test(NAME sr_bridge COMMAND rsf_sr_bridge_test)
set_tests_properties(sr_bridge PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)

# The Rust fixture injects deterministic providers without opening vendor DLLs.
add_executable(rsf_sr_session_test fixture_main.c)
target_compile_definitions(rsf_sr_session_test PRIVATE RSF_FIXTURE_ENTRY=rsf_sr_session_fixture)
target_link_libraries(rsf_sr_session_test PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options)
add_test(NAME sr_session COMMAND rsf_sr_session_test)

add_executable(rsf_motion_resolve_test fixture_main.c)
target_compile_definitions(rsf_motion_resolve_test PRIVATE RSF_FIXTURE_ENTRY=rsf_motion_resolve_fixture)
target_link_libraries(rsf_motion_resolve_test PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options)
add_test(NAME motion_resolve COMMAND rsf_motion_resolve_test)
set_tests_properties(motion_resolve PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)

add_executable(rsf_colour_transport_test fixture_main.c)
target_compile_definitions(rsf_colour_transport_test PRIVATE RSF_FIXTURE_ENTRY=rsf_colour_transport_fixture)
target_link_libraries(rsf_colour_transport_test PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options)
add_test(NAME colour_transport COMMAND rsf_colour_transport_test)
set_tests_properties(colour_transport PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)
rsf_add_rust_static(rsf_translucency_fixtures_rust rsf_rust_translucency_fixtures
    rsf-translucency-fixtures rsf_translucency_fixtures)
add_executable(rsf_native_translucency_test fixture_main.c)
target_compile_definitions(rsf_native_translucency_test PRIVATE RSF_FIXTURE_ENTRY=rsf_native_translucency_fixture)
add_executable(rsf_native_translucency_abi_test fixture_main.c)
target_compile_definitions(rsf_native_translucency_abi_test PRIVATE RSF_FIXTURE_ENTRY=rsf_native_translucency_abi_fixture)
foreach(client rsf_native_translucency_test rsf_native_translucency_abi_test)
    # Only import the Runtime. Its PUBLIC dependencies also include the embedded core.
    target_link_libraries(${client} PRIVATE rsf_translucency_fixtures_rust
        "$<TARGET_LINKER_FILE:rsf_orchestrator>" rsf_build_options d3d11)
    add_dependencies(${client} rsf_orchestrator)
endforeach()
add_test(NAME native_translucency COMMAND rsf_native_translucency_test)
set_tests_properties(native_translucency PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)
add_test(NAME native_translucency_abi COMMAND rsf_native_translucency_abi_test)
add_executable(rsf_native_translucency_abi_embedded_test fixture_main.c)
target_compile_definitions(rsf_native_translucency_abi_embedded_test PRIVATE
    RSF_FIXTURE_ENTRY=rsf_native_translucency_abi_fixture)
target_link_libraries(rsf_native_translucency_abi_embedded_test PRIVATE rsf_translucency_fixtures_rust
    ReScaleFrame::OrchestratorCore rsf_build_options)
add_test(NAME native_translucency_abi_embedded COMMAND rsf_native_translucency_abi_embedded_test)
add_executable(rsf_sr_pipeline_test fixture_main_args.c)
target_compile_definitions(rsf_sr_pipeline_test PRIVATE RSF_FIXTURE_ENTRY=rsf_fixture_sr_pipeline)
target_link_libraries(rsf_sr_pipeline_test PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options)
add_dependencies(rsf_sr_pipeline_test rsf_orchestrator)
add_test(NAME sr_pipeline COMMAND rsf_sr_pipeline_test)
set_tests_properties(sr_pipeline PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)

add_executable(rsf_fg_session_test fixture_main.c)
target_compile_definitions(rsf_fg_session_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_fg_session)
target_link_libraries(rsf_fg_session_test PRIVATE rsf_generation_fixtures_rust rsf_orchestrator rsf_build_options)
add_test(NAME fg_session COMMAND rsf_fg_session_test)
set_tests_properties(fg_session PROPERTIES TIMEOUT 30)

add_executable(rsf_fg12_abi_test fg12_abi.c)
target_link_libraries(rsf_fg12_abi_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME fg12_abi COMMAND rsf_fg12_abi_test)
add_executable(rsf_fg12_abi_embedded_test fg12_abi.c)
target_link_libraries(rsf_fg12_abi_embedded_test PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options)
add_test(NAME fg12_abi_embedded COMMAND rsf_fg12_abi_embedded_test)
add_executable(rsf_sr12_abi_test sr12_abi.c)
target_link_libraries(rsf_sr12_abi_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME sr12_abi COMMAND rsf_sr12_abi_test)
add_executable(rsf_sr12_abi_embedded_test sr12_abi.c)
target_link_libraries(rsf_sr12_abi_embedded_test PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options)
add_test(NAME sr12_abi_embedded COMMAND rsf_sr12_abi_embedded_test)

add_executable(rsf_fg_provider_test fixture_main_args.c)
target_compile_definitions(rsf_fg_provider_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_fg_provider)
add_executable(rsf_fg_choice_test generation/fg_choice_main.c)
target_link_libraries(rsf_fg_choice_test PRIVATE rsf_generation_fixtures_rust rsf_orchestrator rsf_build_options)
add_test(NAME fg_choice COMMAND rsf_fg_choice_test)
target_link_libraries(rsf_fg_provider_test PRIVATE rsf_generation_fixtures_rust rsf_orchestrator rsf_build_options)
add_test(NAME fg_provider COMMAND rsf_fg_provider_test)
set_tests_properties(fg_provider PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_fg_leases_test fixture_main.c)
target_compile_definitions(rsf_fg_leases_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_fg_leases)
target_link_libraries(rsf_fg_leases_test PRIVATE rsf_generation_fixtures_rust rsf_orchestrator rsf_build_options)
add_test(NAME fg_leases COMMAND rsf_fg_leases_test)
set_tests_properties(fg_leases PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_fg_leases_faults_test fixture_main.c)
target_compile_definitions(rsf_fg_leases_faults_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_fg_leases_faults)
target_link_libraries(rsf_fg_leases_faults_test PRIVATE rsf_generation_fixtures_rust rsf_orchestrator rsf_build_options)
add_test(NAME fg_leases_faults COMMAND rsf_fg_leases_faults_test)

add_library(rsf_fg_c_header OBJECT fg_c_header.c)
target_include_directories(rsf_fg_c_header PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_fg_c_header PRIVATE ReScaleFrame::DLSS rsf_build_options)

add_executable(rsf_fg_switching_test fixture_main_args.c)
target_compile_definitions(rsf_fg_switching_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_fg_switching)
add_executable(rsf_d3d11_present_bridge_test d3d11_present_bridge.c)
add_executable(rsf_fg_present_bridge_test fixture_main_args.c)
target_compile_definitions(rsf_fg_present_bridge_test PRIVATE RSF_FIXTURE_ENTRY=rsf_fg_fixture_main)
set_target_properties(rsf_fg_present_bridge_test PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_fg_present_bridge_test PRIVATE rsf_presentation_fixtures_rust rsf_orchestrator_core rsf_build_options)
set_target_properties(rsf_fg_present_bridge_test PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/fixtures/fg/$<CONFIG>")
add_test(NAME shared_fg_present_bridge COMMAND rsf_fg_present_bridge_test)
set_tests_properties(shared_fg_present_bridge PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)
target_compile_definitions(rsf_d3d11_present_bridge_test PRIVATE RSF_STREAMLINE_HOST_BUILD)
# The render scope calls other than create are exported by the rsf-ac7 crate (render_scope.rs).
target_link_libraries(rsf_d3d11_present_bridge_test PRIVATE rsf_presentation_fixtures_rust rsf_orchestrator_core ReScaleFrame::AC7RenderScope rsf_ac7_fixture_rust rsf_build_options d3d11 dxgi dxguid)
# Graphics fixtures must not load game-facing proxy DLLs from the shared artifact directory.
set_target_properties(rsf_d3d11_present_bridge_test PROPERTIES LINKER_LANGUAGE CXX
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/fixtures/fg/$<CONFIG>")
add_test(NAME d3d11_present_bridge COMMAND rsf_d3d11_present_bridge_test)
set_tests_properties(d3d11_present_bridge PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)
add_dependencies(rsf_d3d11_present_bridge_test rsf_proxy_ac7)
add_test(NAME ac7_proxy_startup COMMAND rsf_d3d11_present_bridge_test
    --proxy-smoke $<TARGET_FILE:rsf_proxy_ac7>)
set_tests_properties(ac7_proxy_startup PROPERTIES TIMEOUT 120)
target_link_libraries(rsf_fg_switching_test PRIVATE rsf_generation_fixtures_rust rsf_orchestrator rsf_build_options)
add_test(NAME fg_switching COMMAND rsf_fg_switching_test)
set_tests_properties(fg_switching PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_latency_host_test fixture_main_args.c)
target_compile_definitions(rsf_latency_host_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_latency_host)

# These latency fixtures supply fake FG endpoints while linking the production Rust lease.
set(rsf_vulkan_latency_test_endpoints
    rsf_fgvk_backend=rsf_test_fgvk_backend
    rsf_fgvk_latency_options=rsf_test_fgvk_latency_options
    rsf_fgvk_latency_sleep=rsf_test_fgvk_latency_sleep
    rsf_fgvk_latency_marker=rsf_test_fgvk_latency_marker
    rsf_fgvk_latency_get_status=rsf_test_fgvk_latency_get_status)
add_executable(rsf_native_latency_vulkan_test fixture_main.c ../runtime/orchestrator/src/native_latency_vulkan.c)
target_sources(rsf_native_latency_vulkan_test PRIVATE
    ../runtime/orchestrator/src/latency_amd_vulkan.c
    ../runtime/orchestrator/src/native_latency_streamline_exceptions.c)
target_include_directories(rsf_native_latency_vulkan_test PRIVATE ../runtime/orchestrator/include)
target_compile_definitions(rsf_native_latency_vulkan_test PRIVATE RSF_RUNTIME_BUILD RSF_HAVE_STREAMLINE_VK=1
    RSF_FIXTURE_ENTRY=rsf_native_latency_vulkan_fixture
    ${rsf_vulkan_latency_test_endpoints})
target_link_libraries(rsf_native_latency_vulkan_test PRIVATE rsf_game_sdk_fixtures_rust ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_native_cpu rsf_build_options)
add_test(NAME native_latency_vulkan_contract COMMAND rsf_native_latency_vulkan_test)
find_program(RSF_DOTNET_EXECUTABLE NAMES dotnet)
if(RSF_DOTNET_EXECUTABLE)
    execute_process(COMMAND "${RSF_DOTNET_EXECUTABLE}" --list-sdks OUTPUT_VARIABLE rsf_dotnet_sdks ERROR_QUIET)
    if(rsf_dotnet_sdks MATCHES "(^|\n)10\\.")
        add_test(NAME unity_input_facts_contract COMMAND "${RSF_DOTNET_EXECUTABLE}" run --project
            "${PROJECT_SOURCE_DIR}/tests/unity-input-facts/ReScaleFrame.Unity.InputFacts.Tests.csproj" --configuration Release)
        set_tests_properties(unity_input_facts_contract PROPERTIES TIMEOUT 120)
    endif()
endif()
if(MSVC)
    set(godot_graph_rust_target x86_64-pc-windows-msvc)
else()
    set(godot_graph_rust_target x86_64-pc-windows-gnu)
endif()
set(godot_module "${PROJECT_SOURCE_DIR}/integrations/godot/modules/rescaleframe")
set(godot_graph_object "${CMAKE_CURRENT_BINARY_DIR}/godot_graph_module${CMAKE_CXX_OUTPUT_EXTENSION}")
add_custom_command(OUTPUT "${godot_graph_object}"
    COMMAND "${RSF_RUSTC}" --edition=2024 --crate-type=lib --emit=obj --target ${godot_graph_rust_target}
        --cfg rsf_graph_fixture -Cpanic=abort -Copt-level=2 -Ccodegen-units=1 -Dwarnings
        "${godot_module}/viewport.rs" -o "${godot_graph_object}"
    DEPENDS "${godot_module}/viewport.rs" "${godot_module}/cpu_facts.rs" "${godot_module}/frame.rs"
        "${godot_module}/camera.rs" "${godot_module}/motion.glsl" "${CMAKE_CURRENT_SOURCE_DIR}/godot_graph_handoff.rs"
        "${godot_module}/graph.rs" "${godot_module}/host.rs" "${godot_module}/pass.rs"
        "${godot_module}/motion.rs" "${godot_module}/native.rs" "${godot_module}/packet.rs" "${godot_module}/packet_abi.rs"
        "${godot_module}/present.rs" "${godot_module}/primary.rs"
        "${godot_module}/frame_event_abi.rs" "${PROJECT_SOURCE_DIR}/rust-toolchain.toml"
        "${godot_module}/frame_abi.rs"
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
    VERBATIM)
set_source_files_properties("${godot_graph_object}" PROPERTIES EXTERNAL_OBJECT TRUE GENERATED TRUE)
add_executable(rsf_godot_graph_handoff_test godot_graph_handoff.cpp "${godot_graph_object}")
target_include_directories(rsf_godot_graph_handoff_test PRIVATE godot_graph_stubs ../integrations/godot/native ../runtime/orchestrator/include)
target_link_libraries(rsf_godot_graph_handoff_test PRIVATE ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_build_options)
add_test(NAME godot_graph_handoff_contract COMMAND rsf_godot_graph_handoff_test)
if(RSF_HAVE_VULKAN AND RSF_HAVE_FFX_DX12_SR)
    add_executable(rsf_native_sr_fsr4_vulkan_test native_sr_fsr4_vulkan_main.c)
    set_target_properties(rsf_native_sr_fsr4_vulkan_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON
        LINKER_LANGUAGE CXX)
    if(MINGW)
        target_link_options(rsf_native_sr_fsr4_vulkan_test PRIVATE -municode)
    endif()
    target_link_libraries(rsf_native_sr_fsr4_vulkan_test PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME native_sr_fsr4_vulkan_contract COMMAND rsf_native_sr_fsr4_vulkan_test)
    add_executable(rsf_native_sr_fsr4_state_test fixture_main.c)
    target_compile_definitions(rsf_native_sr_fsr4_state_test PRIVATE RSF_FIXTURE_ENTRY=rsf_fixture_fsr4sr_state)
    set_target_properties(rsf_native_sr_fsr4_state_test PROPERTIES LINKER_LANGUAGE CXX)
    target_include_directories(rsf_native_sr_fsr4_state_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_native_sr_fsr4_state_test PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME native_sr_fsr4_state_contract COMMAND rsf_native_sr_fsr4_state_test)
endif()
if(RSF_HAVE_VULKAN AND RSF_HAVE_XESS_FG)
    add_executable(rsf_native_fg_xess_vulkan_test native_fg_xess_vulkan_interop.c)
    set_target_properties(rsf_native_fg_xess_vulkan_test PROPERTIES LINKER_LANGUAGE CXX)
    if(MINGW)
        target_link_options(rsf_native_fg_xess_vulkan_test PRIVATE -municode)
    endif()
    target_link_libraries(rsf_native_fg_xess_vulkan_test PRIVATE rsf_vulkan_fg_fixtures_rust ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME native_fg_xess_vulkan_contract COMMAND rsf_native_fg_xess_vulkan_test)
endif()
if(RSF_HAVE_VULKAN)
    add_executable(rsf_ffx_vulkan_wsi_dispatch_test ffx_vulkan_wsi_dispatch.c)
    set_target_properties(rsf_ffx_vulkan_wsi_dispatch_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON
        LINKER_LANGUAGE CXX)
    target_include_directories(rsf_ffx_vulkan_wsi_dispatch_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_ffx_vulkan_wsi_dispatch_test PRIVATE rsf_vulkan_fg_fixtures_rust rsf_build_options)
    add_test(NAME ffx_vulkan_wsi_dispatch COMMAND rsf_ffx_vulkan_wsi_dispatch_test)
    if(RSF_HAVE_FFX_VK)
        # The scenarios replace the SDK module through the host table, so the
        # production leaf is linked without RSF_HAVE_FFX_VK and its host.
        add_executable(rsf_native_ffx_vulkan_fixture vulkan-fg/ffx_fixture.c
            ../runtime/orchestrator/src/native_fg_vulkan.c)
        set_target_properties(rsf_native_ffx_vulkan_fixture PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON
            LINKER_LANGUAGE CXX)
        target_include_directories(rsf_native_ffx_vulkan_fixture PRIVATE ../runtime/orchestrator/include)
        target_compile_definitions(rsf_native_ffx_vulkan_fixture PRIVATE RSF_RUNTIME_BUILD)
        target_link_libraries(rsf_native_ffx_vulkan_fixture PRIVATE rsf_vulkan_fg_fixtures_rust rsf_orchestrator_rust
            ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::FrameDump rsf_build_options)
        add_test(NAME native_ffx_vulkan_reset_contract COMMAND rsf_native_ffx_vulkan_fixture 0)
        add_test(NAME native_ffx_vulkan_submission_contract COMMAND rsf_native_ffx_vulkan_fixture 1)
        add_test(NAME native_ffx_vulkan_wsi_route_contract COMMAND rsf_native_ffx_vulkan_fixture 2)
    endif()
endif()
target_link_libraries(rsf_latency_host_test PRIVATE rsf_generation_fixtures_rust rsf_orchestrator rsf_build_options)
add_test(NAME latency_host COMMAND rsf_latency_host_test)
set_tests_properties(latency_host PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_render_links_test fixture_main.c)
target_compile_definitions(rsf_render_links_test PRIVATE RSF_FIXTURE_ENTRY=rsf_test_render_links)
target_link_libraries(rsf_render_links_test PRIVATE rsf_game_sdk_fixtures_rust rsf_orchestrator rsf_build_options)
add_test(NAME render_links COMMAND rsf_render_links_test)
set_tests_properties(render_links PROPERTIES TIMEOUT 120)

rsf_ac7_fixture_object(ac7_scope_fixture_object ac7_render_scope ac7_render_scope_fixture_abi.rs)
add_executable(rsf_ac7_render_scope_test ac7_render_scope.c "${ac7_scope_fixture_object}")
target_link_libraries(rsf_ac7_render_scope_test PRIVATE ReScaleFrame::AC7RenderScope rsf_ac7_rust ReScaleFrame::GraphicsLeaves rsf_build_options)
add_test(NAME ac7_render_scope COMMAND rsf_ac7_render_scope_test)
set_tests_properties(ac7_render_scope PROPERTIES TIMEOUT 120)

# Explicit hardware experiment, never run by routine CTest verification.
rsf_add_rust_static(rsf_exposure_probe_rust rsf_exposure_probe_rust_build rsf-exposure-probe rsf_exposure_probe)
# The probe archive supplies the C main symbol. Keep the platform console CRT startup.
add_executable(rsf_exposure_probe EXCLUDE_FROM_ALL ../tools/exposure_probe/renderdoc.c)
if(MSVC)
    target_link_options(rsf_exposure_probe PRIVATE /SUBSYSTEM:CONSOLE)
endif()
set_target_properties(rsf_exposure_probe PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON)
target_link_libraries(rsf_exposure_probe PRIVATE rsf_exposure_probe_rust ReScaleFrame::DLSS ReScaleFrame::Graphics minhook rsf_build_options)
if(EXISTS "${RSF_RENDERDOC_INCLUDE}/renderdoc_app.h")
    target_include_directories(rsf_exposure_probe SYSTEM PRIVATE "${RSF_RENDERDOC_INCLUDE}")
    target_compile_definitions(rsf_exposure_probe PRIVATE RSF_PROBE_RENDERDOC=1)
endif()
set(RSF_NGX_PROBE_INCLUDE "" CACHE PATH "Optional NGX SDK headers for standalone probe parameter tracing")
if(EXISTS "${RSF_NGX_PROBE_INCLUDE}/nvsdk_ngx_defs.h")
    target_sources(rsf_exposure_probe PRIVATE ../tools/exposure_probe/ngx.c)
    target_include_directories(rsf_exposure_probe SYSTEM PRIVATE "${RSF_NGX_PROBE_INCLUDE}")
    target_compile_definitions(rsf_exposure_probe PRIVATE RSF_PROBE_NGX_PARAMS=1)
endif()

set(ac7_view_fixture_object "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>/rsf_ac7_view_fixture.obj")
add_custom_command(OUTPUT "${ac7_view_fixture_object}"
    COMMAND "${CMAKE_COMMAND}" -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>"
    COMMAND "${ac7_fixture_rustc}" --edition 2024 --target "${ac7_fixture_target}"
        --crate-name rsf_ac7_view_fixture --crate-type lib --emit obj -C codegen-units=1
        -C panic=abort -C opt-level=$<IF:$<CONFIG:Debug>,0,3> ${RSF_FIXTURE_EXTERNS}
        -o "${ac7_view_fixture_object}" "${CMAKE_CURRENT_SOURCE_DIR}/ac7_view_fixture.rs"
    DEPENDS ac7_view_fixture.rs ac7_graph_replay.rs ac7_graph_replay_abi.rs rsf_rust_fixture_rlibs
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
    VERBATIM)
set_source_files_properties("${ac7_view_fixture_object}" PROPERTIES GENERATED TRUE EXTERNAL_OBJECT TRUE)
# The C launcher hands the replay helpers above, rsf_native_sr_evaluate, the render scope create
# call and the native renderer's call table to the Rust fixture (ac7_native_graph.rs), which the
# rsf-ac7 crate builds with its warp-fixture feature beside games/ac7/src/native_renderer.rs.
add_executable(rsf_ac7_native_graph_test ac7_native_graph.c "${ac7_view_fixture_object}")
set_target_properties(rsf_ac7_native_graph_test PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_ac7_native_graph_test PRIVATE ReScaleFrame::AC7RenderScope rsf_ac7_fixture_rust ReScaleFrame::GraphicsLeaves rsf_orchestrator minhook rsf_build_options)
add_test(NAME ac7_native_graph COMMAND rsf_ac7_native_graph_test)
set_tests_properties(ac7_native_graph PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_gl_shared_surface_test presentation/gl_shared_surface_main.c)
set_target_properties(rsf_gl_shared_surface_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON
    LINKER_LANGUAGE CXX)
target_link_libraries(rsf_gl_shared_surface_test PRIVATE rsf_presentation_fixtures_rust rsf_minecraft_rust ReScaleFrame::OrchestratorCore rsf_build_options opengl32)
add_test(NAME gl_shared_surface COMMAND rsf_gl_shared_surface_test)
set_tests_properties(gl_shared_surface PROPERTIES TIMEOUT 60 SKIP_RETURN_CODE 77)

add_executable(rsf_native_vulkan_test native_sr_vulkan_main.c)
set_target_properties(rsf_native_vulkan_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON LINKER_LANGUAGE CXX)
target_link_libraries(rsf_native_vulkan_test PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
add_test(NAME native_vulkan_contract COMMAND rsf_native_vulkan_test)
add_library(rsf_native_split_c_header OBJECT native_sr_split_vulkan.c)
target_include_directories(rsf_native_split_c_header PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_native_split_c_header PRIVATE ReScaleFrame::Contract ReScaleFrame::GameSDK rsf_build_options)
add_test(NAME native_vulkan_device COMMAND rsf_native_vulkan_test --device)
add_test(NAME native_vulkan_dlss_device COMMAND rsf_native_vulkan_test --dlss)
set_tests_properties(native_vulkan_device native_vulkan_dlss_device PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_native_xess_vulkan_test native_xess_vulkan_main.c)
set_target_properties(rsf_native_xess_vulkan_test PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_native_xess_vulkan_test PRIVATE rsf_vulkan_fg_fixtures_rust ReScaleFrame::NativeVulkan rsf_build_options)
if(RSF_HAVE_VULKAN AND RSF_HAVE_XESS_VK)
    target_compile_definitions(rsf_native_xess_vulkan_test PRIVATE RSF_TEST_VULKAN=1)
endif()
add_test(NAME native_xess_vulkan_contract COMMAND rsf_native_xess_vulkan_test)
if(RSF_HAVE_VULKAN AND RSF_HAVE_XESS_VK AND RSF_HAVE_FFX_VK)
    add_executable(rsf_native_xess_input_range_test fixture_main.c)
    set_target_properties(rsf_native_xess_input_range_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON LINKER_LANGUAGE CXX)
    target_compile_definitions(rsf_native_xess_input_range_test PRIVATE RSF_FIXTURE_ENTRY=rsf_fixture_xess_input_range)
    target_link_libraries(rsf_native_xess_input_range_test PRIVATE rsf_vulkan_fg_fixtures_rust ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME native_xess_input_range_contract COMMAND rsf_native_xess_input_range_test)
endif()
add_executable(rsf_native_fg_vulkan_dispatch_test vulkan-fg/dispatch_fixture.c
    ../runtime/orchestrator/src/native_fg_vulkan_dispatch.c)
set_target_properties(rsf_native_fg_vulkan_dispatch_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON LINKER_LANGUAGE CXX)
target_compile_definitions(rsf_native_fg_vulkan_dispatch_test PRIVATE RSF_RUNTIME_BUILD)
target_include_directories(rsf_native_fg_vulkan_dispatch_test PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_native_fg_vulkan_dispatch_test PRIVATE rsf_vulkan_fg_fixtures_rust rsf_orchestrator_rust ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_build_options)
add_test(NAME native_fg_vulkan_dispatch_contract COMMAND rsf_native_fg_vulkan_dispatch_test)
add_executable(rsf_native_fg_vulkan_dispatch_xess_test vulkan-fg/dispatch_fixture.c
    ../runtime/orchestrator/src/native_fg_vulkan_dispatch.c)
set_target_properties(rsf_native_fg_vulkan_dispatch_xess_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON LINKER_LANGUAGE CXX)
target_compile_definitions(rsf_native_fg_vulkan_dispatch_xess_test PRIVATE RSF_RUNTIME_BUILD RSF_HAVE_XESS_FG_VK=1)
target_include_directories(rsf_native_fg_vulkan_dispatch_xess_test PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_native_fg_vulkan_dispatch_xess_test PRIVATE rsf_vulkan_fg_fixtures_rust rsf_orchestrator_rust ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_build_options)
add_test(NAME native_fg_vulkan_dispatch_xess_contract COMMAND rsf_native_fg_vulkan_dispatch_xess_test)
add_executable(rsf_native_fg_vulkan_dispatch_fsr4_test vulkan-fg/dispatch_fixture.c
    ../runtime/orchestrator/src/native_fg_vulkan_dispatch.c)
set_target_properties(rsf_native_fg_vulkan_dispatch_fsr4_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON LINKER_LANGUAGE CXX)
target_compile_definitions(rsf_native_fg_vulkan_dispatch_fsr4_test PRIVATE RSF_RUNTIME_BUILD RSF_HAVE_FSR4_FG_VK=1)
target_include_directories(rsf_native_fg_vulkan_dispatch_fsr4_test PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_native_fg_vulkan_dispatch_fsr4_test PRIVATE rsf_vulkan_fg_fixtures_rust rsf_orchestrator_rust ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_build_options)
# The Runtime archive defines the production rsf_fsr4gvk_* entries (vulkan_fsr4_fg.rs), so the
# fixture's fakes carry a fixture prefix and this build of the dispatch calls them by that name.
foreach(rsf_fsr4_entry create enable_options max_generated status images acquire prepare hudless submitted present_source present destroy drain)
    target_compile_definitions(rsf_native_fg_vulkan_dispatch_fsr4_test PRIVATE
        rsf_fsr4gvk_${rsf_fsr4_entry}=rsf_fixture_fsr4gvk_${rsf_fsr4_entry})
endforeach()
add_test(NAME native_fg_vulkan_dispatch_fsr4_contract COMMAND rsf_native_fg_vulkan_dispatch_fsr4_test)
if(RSF_HAVE_VULKAN AND RSF_HAVE_FFX_DX12_FG)
    add_executable(rsf_native_fsr4_fg_bridge_test presentation/fsr4_fg_bridge_main.c)
    set_target_properties(rsf_native_fsr4_fg_bridge_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON
        LINKER_LANGUAGE CXX)
    if(MINGW)
        target_link_options(rsf_native_fsr4_fg_bridge_test PRIVATE -municode)
    endif()
    target_link_libraries(rsf_native_fsr4_fg_bridge_test PRIVATE rsf_presentation_fixtures_rust
        ReScaleFrame::NativeVulkan rsf_build_options dxgi)
    add_test(NAME native_fsr4_fg_bridge_contract COMMAND rsf_native_fsr4_fg_bridge_test)
    add_executable(rsf_native_fsr4_fg_state_test fixture_main.c)
    set_target_properties(rsf_native_fsr4_fg_state_test PROPERTIES LINKER_LANGUAGE CXX)
    target_compile_definitions(rsf_native_fsr4_fg_state_test PRIVATE RSF_FIXTURE_ENTRY=rsf_fixture_fsr4fg_wsi_state)
    target_link_libraries(rsf_native_fsr4_fg_state_test PRIVATE rsf_vulkan_fg_fixtures_rust
        ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_orchestrator_rust rsf_build_options)
    add_test(NAME native_fsr4_fg_state_contract COMMAND rsf_native_fsr4_fg_state_test)
endif()
add_test(NAME native_xess_vulkan_device COMMAND rsf_native_xess_vulkan_test --device)
set_tests_properties(native_xess_vulkan_device PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

if(RSF_HAVE_VULKAN)
    add_executable(rsf_streamline_vulkan_dispatch_test streamline_vulkan_dispatch.cpp)
    target_include_directories(rsf_streamline_vulkan_dispatch_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_streamline_vulkan_dispatch_test PRIVATE rsf_vulkan_fg_fixtures_rust rsf_build_options)
    add_test(NAME streamline_vulkan_dispatch_contract COMMAND rsf_streamline_vulkan_dispatch_test)
    add_executable(rsf_native_antilag_vulkan_test fixture_main.c
        ../runtime/orchestrator/src/native_latency_vulkan.c ../runtime/orchestrator/src/latency_amd_vulkan.c)
    target_compile_definitions(rsf_native_antilag_vulkan_test PRIVATE RSF_RUNTIME_BUILD RSF_HAVE_ANTILAG_VK=1
        RSF_FIXTURE_ENTRY=rsf_native_antilag_vulkan_fixture
        ${rsf_vulkan_latency_test_endpoints})
    target_include_directories(rsf_native_antilag_vulkan_test PRIVATE ../runtime/orchestrator/include)
    target_link_libraries(rsf_native_antilag_vulkan_test PRIVATE rsf_game_sdk_fixtures_rust ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_native_cpu rsf_build_options)
    add_test(NAME native_antilag_vulkan_contract COMMAND rsf_native_antilag_vulkan_test)
    set_tests_properties(native_antilag_vulkan_contract PROPERTIES TIMEOUT 20)
endif()

if(RSF_HAVE_VULKAN AND RSF_HAVE_STREAMLINE)
    add_executable(rsf_streamline_selected_device_test streamline_selected_device.cpp)
    get_filename_component(rsf_streamline_source_root "${RSF_STREAMLINE_INCLUDE}" DIRECTORY)
    target_include_directories(rsf_streamline_selected_device_test SYSTEM PRIVATE "${rsf_streamline_source_root}" "${RSF_STREAMLINE_INCLUDE}")
    target_link_libraries(rsf_streamline_selected_device_test PRIVATE rsf_vulkan_fg_fixtures_rust rsf_build_options)
    add_test(NAME streamline_selected_device_contract COMMAND rsf_streamline_selected_device_test)
    # The production DLSS-G table and its Rust leaves, with the Rust fixture's
    # Streamline session and SDK entries.
    add_executable(rsf_native_fg_dlss_vulkan_test fixture_main.c
        ../runtime/orchestrator/src/native_fg_dlss_vulkan.c)
    set_target_properties(rsf_native_fg_dlss_vulkan_test PROPERTIES LINKER_LANGUAGE CXX)
    target_compile_definitions(rsf_native_fg_dlss_vulkan_test PRIVATE RSF_RUNTIME_BUILD RSF_HAVE_STREAMLINE_VK=1
        RSF_FIXTURE_ENTRY=rsf_fixture_dlss_run)
    target_include_directories(rsf_native_fg_dlss_vulkan_test PRIVATE ../runtime/orchestrator/include)
    target_include_directories(rsf_native_fg_dlss_vulkan_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_native_fg_dlss_vulkan_test PRIVATE rsf_vulkan_fg_fixtures_rust rsf_orchestrator_rust ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::FrameDump rsf_build_options)
    add_test(NAME native_fg_dlss_vulkan_contract COMMAND rsf_native_fg_dlss_vulkan_test)
endif()


add_executable(rsf_module_dump_v2 loader/module_dump_rtti_fixture.cpp)
target_link_libraries(rsf_module_dump_v2 PRIVATE
    rsf_loader_fixtures_rust ReScaleFrame::LoaderDiagnostics rsf_build_options bcrypt)
add_test(NAME module_dump_v2 COMMAND rsf_module_dump_v2 "${CMAKE_CURRENT_BINARY_DIR}")
set_tests_properties(module_dump_v2 PROPERTIES TIMEOUT 120)

# Vendor call census: fake vendor DLLs named like the real ones, a test that installs the census
# and calls them, and a real DXGI/D3D12 (WARP) path. MSVC only, like the library.
if(TARGET rsf_vendor_census)
    function(rsf_census_fixture target define output)
        string(TOLOWER "${define}" fixture)
        add_library(${target} SHARED loader/vendor_census_fixture.c "loader/${fixture}.def")
        target_compile_definitions(${target} PRIVATE ${define})
        target_link_libraries(${target} PRIVATE rsf_loader_fixtures_rust rsf_build_options)
        set_target_properties(${target} PROPERTIES OUTPUT_NAME "${output}")
        # Identical-function folding or incremental thunks would put two exports on one address.
        target_link_options(${target} PRIVATE /OPT:NOICF /INCREMENTAL:NO)
    endfunction()
    rsf_census_fixture(rsf_census_fx_fsr2 FIXTURE_FSR2 ffx_fsr2_api_x64)
    rsf_census_fixture(rsf_census_fx_ngx FIXTURE_NGX _nvngx)
    rsf_census_fixture(rsf_census_fx_nvapi FIXTURE_NVAPI rsf_fixture_nvapi)
    rsf_census_fixture(rsf_census_fx_xess FIXTURE_XESS libxess)
    rsf_census_fixture(rsf_census_fx_vk FIXTURE_VK rsf_fixture_vulkan)
    rsf_census_fixture(rsf_census_fx_sl FIXTURE_SL sl.interposer)
    rsf_census_fixture(rsf_census_fx_ffx FIXTURE_FFX amd_fidelityfx_dx12)
    rsf_census_fixture(rsf_census_fx_creation FIXTURE_CREATION rsf_fixture_creation)

    add_library(rsf_census_asi SHARED loader/vendor_census_asi.c loader/vendor_census_asi.def)
    target_link_libraries(rsf_census_asi PRIVATE
        rsf_loader_fixtures_rust ReScaleFrame::VendorCensus rsf_build_options)

    add_executable(rsf_vendor_census_test loader/vendor_census_native.c loader/vendor_census_ngx_platform.cpp)
    target_link_libraries(rsf_vendor_census_test PRIVATE
        rsf_loader_fixtures_rust ReScaleFrame::VendorCensus rsf_census_fx_fsr2 rsf_build_options)
    add_dependencies(rsf_vendor_census_test rsf_census_fx_ngx rsf_census_fx_nvapi rsf_census_fx_xess rsf_census_fx_vk rsf_census_fx_sl rsf_census_fx_ffx rsf_census_fx_creation rsf_census_asi)
    add_test(NAME vendor_census COMMAND rsf_vendor_census_test functional "${CMAKE_CURRENT_BINARY_DIR}/census-functional")
    add_test(NAME vendor_census_budget COMMAND rsf_vendor_census_test budget "${CMAKE_CURRENT_BINARY_DIR}/census-budget")
    add_test(NAME vendor_census_vulkan_streamline COMMAND rsf_vendor_census_test vulkan "${CMAKE_CURRENT_BINARY_DIR}/census-vulkan")
    add_test(NAME vendor_census_fidelityfx_api COMMAND rsf_vendor_census_test ffx "${CMAKE_CURRENT_BINARY_DIR}/census-ffx")
    add_test(NAME vendor_census_dllmain COMMAND rsf_vendor_census_test dllmain "${CMAKE_CURRENT_BINARY_DIR}/census-dllmain")
    add_test(NAME vendor_census_creation_identity COMMAND rsf_vendor_census_test creation "${CMAKE_CURRENT_BINARY_DIR}/census-creation")
    set_tests_properties(vendor_census_creation_identity PROPERTIES TIMEOUT 30)
    set_tests_properties(vendor_census vendor_census_budget vendor_census_vulkan_streamline vendor_census_fidelityfx_api vendor_census_dllmain PROPERTIES TIMEOUT 120)

    if(TARGET rsf_asi_rdr2)
        rsf_add_rust_static(rsf_rdr2_fixtures_rust rsf_rust_rdr2_fixtures rsf-rdr2 rsf_rdr2
            --features camera-fixture,capture-fixture)
        add_executable(rsf_rdr2_camera fixture_main.c)
        target_compile_definitions(rsf_rdr2_camera PRIVATE RSF_FIXTURE_ENTRY=rsf_rdr2_camera_fixture)
        target_link_libraries(rsf_rdr2_camera PRIVATE rsf_rdr2_fixtures_rust minhook user32 rsf_build_options)
        add_test(NAME rdr2_camera COMMAND rsf_rdr2_camera "${CMAKE_CURRENT_SOURCE_DIR}/fixtures/rdr2-camera-v1.bin")
        add_executable(rsf_rdr2_capture rdr2_capture_main.c rdr2_capture_ngx.c ../games/rdr2/src/capture_ngx.c)
        target_compile_definitions(rsf_rdr2_capture PRIVATE RSF_RDR2_CAPTURE_FIXTURE)
        target_link_libraries(rsf_rdr2_capture PRIVATE rsf_rdr2_fixtures_rust ReScaleFrame::VendorCensus minhook
            d3d12 dxgi d3dcompiler user32 rsf_build_options)
        add_dependencies(rsf_rdr2_capture rsf_census_fx_ngx)
        add_test(NAME rdr2_capture COMMAND rsf_rdr2_capture "${CMAKE_CURRENT_BINARY_DIR}/rdr2-capture")
        add_test(NAME rdr2_capture_rgb10 COMMAND rsf_rdr2_capture "${CMAKE_CURRENT_BINARY_DIR}/rdr2-capture-rgb10" rgb10)
        set_tests_properties(rdr2_capture rdr2_capture_rgb10 PROPERTIES TIMEOUT 60)
    endif()
    # Hardware and desktop reproduction of the census next to an injected overlay; run by hand.
    add_executable(rsf_census_present_coexistence EXCLUDE_FROM_ALL loader/census_present_main.c)
    target_link_libraries(rsf_census_present_coexistence PRIVATE
        rsf_loader_fixtures_rust ReScaleFrame::VendorCensus d3d12 dxgi user32 rsf_build_options)

    # The census restates vendor structure layouts so it builds without the vendor headers. These
    # tests compare the restated offsets with the headers where an untracked checkout provides them.
    function(rsf_census_layout_check suffix define probe)
        if(EXISTS "${probe}")
            add_executable(rsf_vendor_census_layouts_${suffix} loader/vendor_layout_facts.c)
            target_compile_definitions(rsf_vendor_census_layouts_${suffix} PRIVATE ${define})
            # Streamline and FSR 2 declare C++ types; their facts come from the C++ leaf.
            if(suffix STREQUAL "sl" OR suffix STREQUAL "fsr2")
                target_sources(rsf_vendor_census_layouts_${suffix} PRIVATE loader/vendor_layout_facts.cpp)
                target_compile_definitions(rsf_vendor_census_layouts_${suffix} PRIVATE RSF_CENSUS_LAYOUT_CXX)
            endif()
            target_include_directories(rsf_vendor_census_layouts_${suffix} SYSTEM PRIVATE ${ARGN})
            target_link_libraries(rsf_vendor_census_layouts_${suffix} PRIVATE
                rsf_loader_fixtures_rust rsf_build_options)
            add_test(NAME vendor_census_layouts_${suffix} COMMAND rsf_vendor_census_layouts_${suffix})
        else()
            message(STATUS "vendor census layout check ${suffix} skipped: ${probe} not found")
        endif()
    endfunction()
    set(RSF_VS "${CMAKE_SOURCE_DIR}/vendor")
    rsf_census_layout_check(sl RSF_CENSUS_LAYOUT_SL "${RSF_VS}/streamline/include/sl.h" "${RSF_VS}/streamline/include")
    rsf_census_layout_check(ffx RSF_CENSUS_LAYOUT_FFX "${RSF_VS}/fidelityfx/Kits/FidelityFX/api/include/ffx_api.h"
        "${RSF_VS}/fidelityfx/Kits/FidelityFX/api/include"
        "${RSF_VS}/fidelityfx/Kits/FidelityFX/upscalers/include"
        "${RSF_VS}/fidelityfx/Kits/FidelityFX/framegeneration/include")
    rsf_census_layout_check(xess RSF_CENSUS_LAYOUT_XESS "${RSF_VS}/xess/inc/xess/xess_d3d12.h" "${RSF_VS}/xess/inc/xess")
    rsf_census_layout_check(fsr2 RSF_CENSUS_LAYOUT_FSR2
        "${CMAKE_SOURCE_DIR}/references/OptiScaler/OptiScaler/include/fsr2_212/ffx_fsr2.h"
        "${CMAKE_SOURCE_DIR}/references/OptiScaler/OptiScaler/include/fsr2_212")
    if(RSF_FRAME_DUMP_VULKAN_INCLUDE)
        rsf_census_layout_check(vk RSF_CENSUS_LAYOUT_VK "${RSF_FRAME_DUMP_VULKAN_INCLUDE}/vulkan/vulkan.h" "${RSF_FRAME_DUMP_VULKAN_INCLUDE}")
    endif()
    foreach(rsf_layout IN ITEMS sl ffx xess fsr2 vk)
        if(TEST vendor_census_layouts_${rsf_layout})
            set_tests_properties(vendor_census_layouts_${rsf_layout} PROPERTIES SKIP_RETURN_CODE 77)
        endif()
    endforeach()
endif()

add_executable(rsf_sharpening_test fixture_main.c)
target_compile_definitions(rsf_sharpening_test PRIVATE RSF_FIXTURE_ENTRY=rsf_sharpening_fixture)
target_link_libraries(rsf_sharpening_test PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options)
add_test(NAME sharpening COMMAND rsf_sharpening_test)
set_tests_properties(sharpening PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)

add_executable(rsf_sharpening_settings_test sharpening_settings_native.c)
set_target_properties(rsf_sharpening_settings_test PROPERTIES LINKER_LANGUAGE CXX)
# The settings fixture and rsf_ffx_sharpening are Runtime internals, not DLL exports.
target_link_libraries(rsf_sharpening_settings_test PRIVATE rsf_orchestrator_rust rsf_build_options)
if(EXISTS "${CMAKE_SOURCE_DIR}/vendor/fidelityfx/Kits/FidelityFX/upscalers/include/ffx_upscale.h")
    target_include_directories(rsf_sharpening_settings_test SYSTEM PRIVATE "${CMAKE_SOURCE_DIR}/vendor/fidelityfx/Kits/FidelityFX/upscalers/include")
    target_compile_definitions(rsf_sharpening_settings_test PRIVATE RSF_SHARPENING_FFX_HEADER)
endif()
add_test(NAME sharpening_settings COMMAND rsf_sharpening_settings_test)

add_executable(rsf_sharpening_d3d12_test fixture_main.c)
target_compile_definitions(rsf_sharpening_d3d12_test PRIVATE RSF_FIXTURE_ENTRY=rsf_sharpening_d3d12_fixture)
target_link_libraries(rsf_sharpening_d3d12_test PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options)
add_test(NAME sharpening_d3d12 COMMAND rsf_sharpening_d3d12_test)
set_tests_properties(sharpening_d3d12 PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)

if(RSF_HAVE_VULKAN)
    add_executable(rsf_sharpening_vulkan_test fixture_main.c)
    target_compile_definitions(rsf_sharpening_vulkan_test PRIVATE RSF_FIXTURE_ENTRY=rsf_sharpening_vulkan_fixture)
    target_link_libraries(rsf_sharpening_vulkan_test PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME sharpening_vulkan COMMAND rsf_sharpening_vulkan_test)
    set_tests_properties(sharpening_vulkan PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)
    add_executable(rsf_sharpening_vulkan_cpu_test fixture_main.c)
    target_compile_definitions(rsf_sharpening_vulkan_cpu_test PRIVATE RSF_FIXTURE_ENTRY=rsf_sharpening_vulkan_cpu_fixture)
    target_link_libraries(rsf_sharpening_vulkan_cpu_test PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME sharpening_vulkan_cpu COMMAND rsf_sharpening_vulkan_cpu_test)
endif()

if(TARGET rsf_godot AND TARGET rsf_unity_package)
    set(rsf_engine_sharpening_exe "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>/rsf_engine_sharpening_test.exe")
    set(rsf_engine_sharpening_source "${CMAKE_CURRENT_SOURCE_DIR}/engine_sharpening.rs")
    add_custom_command(OUTPUT "${rsf_engine_sharpening_exe}"
        COMMAND "${CMAKE_COMMAND}" -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>"
        COMMAND "${RSF_TEST_RUSTC}" --edition=2024 --target "${rsf_test_rust_target}"
            -D warnings ${RSF_FIXTURE_EXTERNS} "${rsf_engine_sharpening_source}"
            -o "${rsf_engine_sharpening_exe}"
        DEPENDS "${rsf_engine_sharpening_source}" engine_sharpening_abi.rs rsf_rust_fixture_rlibs
        WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
        VERBATIM)
    add_custom_target(rsf_engine_sharpening_test ALL DEPENDS "${rsf_engine_sharpening_exe}")
    add_dependencies(rsf_engine_sharpening_test rsf_godot rsf_unity_package)
    add_test(NAME engine_sharpening_settings COMMAND ${CMAKE_CROSSCOMPILING_EMULATOR}
        "${rsf_engine_sharpening_exe}" "$<TARGET_FILE:rsf_godot>" "$<TARGET_FILE:rsf_unity_package>")
endif()


rsf_add_capture_fixture(rsf_capture_metadata rsf_fixture_capture_metadata capture_fixture_main.c)
target_link_libraries(rsf_capture_metadata PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options windowscodecs ole32)
add_test(NAME capture_metadata_contract COMMAND rsf_capture_metadata "${CMAKE_CURRENT_BINARY_DIR}")

add_executable(rsf_input_receipts_test fixture_main.c)
target_compile_definitions(rsf_input_receipts_test PRIVATE RSF_FIXTURE_ENTRY=rsf_input_receipts_fixture)
target_link_libraries(rsf_input_receipts_test PRIVATE rsf_game_sdk_fixtures_rust rsf_native_cpu rsf_build_options)
add_test(NAME input_receipts_contract COMMAND rsf_input_receipts_test)
if(EXISTS "${RSF_XESS_INCLUDE}/xell/xell_d3d12.h" AND
   EXISTS "${RSF_XESS_INCLUDE}/xess_fg/xefg_swapchain_d3d12.h")
    add_executable(rsf_xell_sleep_concurrency fixture_main.c)
    target_compile_definitions(rsf_xell_sleep_concurrency PRIVATE RSF_FIXTURE_ENTRY=rsf_xell_fixture_run)
    target_link_libraries(rsf_xell_sleep_concurrency PRIVATE rsf_game_sdk_fixtures_rust rsf_orchestrator_rust rsf_build_options)
    add_test(NAME xell_sleep_concurrency_contract COMMAND rsf_xell_sleep_concurrency)
    set_tests_properties(xell_sleep_concurrency_contract PROPERTIES TIMEOUT 10)
endif()

# These executables call the fixture exports of the Rust orchestrator, which only the test-fixtures archive carries
# (runtime/orchestrator/CMakeLists.txt).
foreach(rsf_fixture_target
        rsf_ui_layer rsf_fullscreen_pass rsf_native_reinsert rsf_depth_scale rsf_auxiliary_sr rsf_sampler_bias
        rsf_resource_roles rsf_motion_decode rsf_d3d11_state rsf_motion_resolve_test rsf_colour_transport_test
        rsf_sharpening_test rsf_sharpening_d3d12_test rsf_sharpening_vulkan_test rsf_sharpening_vulkan_cpu_test
        rsf_sharpening_settings_test rsf_exposure_normalization_test rsf_native_vulkan_test rsf_dlss_backend
        rsf_dlss_sr_layout rsf_sr_session_test rsf_sr_bridge_test rsf_sr_pipeline_test rsf_sr_backend_switch_device_test
        rsf_shader_patch_test rsf_shader_patch_device_test rsf_native_sr_fsr4_state_test rsf_native_sr_fsr4_vulkan_test
        rsf_native_fsr4_fg_state_test rsf_native_fg_cpu_test)
    if(TARGET ${rsf_fixture_target})
        set_target_properties(${rsf_fixture_target} PROPERTIES RSF_ORCHESTRATOR_FIXTURES ON)
        add_dependencies(${rsf_fixture_target} rsf_rust_core_fixtures)
    endif()
endforeach()
