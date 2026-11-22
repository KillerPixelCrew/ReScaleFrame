add_library(rsf_sdk_c_header OBJECT sdk_c_header.c)
include("${CMAKE_CURRENT_SOURCE_DIR}/d3d11_host_rendered_fg_device.cmake")
# Explicit regression investigation only: requires a scratch vendor-SDK root and NVIDIA GPU.
add_executable(rsf_d3d11_host_shared_sr_device EXCLUDE_FROM_ALL d3d11_host_shared_sr_device.cpp)
target_link_libraries(rsf_d3d11_host_shared_sr_device PRIVATE rsf_orchestrator rsf_build_options d3d11 dxgi)
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
add_executable(rsf_shader_patch_test shader_patch_main.c)
target_link_libraries(rsf_shader_patch_test PRIVATE rsf_shader_patch rsf_build_options)
add_executable(rsf_shader_patch_device_test shader_patch_main.c)
target_compile_definitions(rsf_shader_patch_device_test PRIVATE RSF_SHADER_PATCH_DEVICE_FIXTURE)
target_link_libraries(rsf_shader_patch_device_test PRIVATE rsf_shader_patch rsf_build_options)
add_test(NAME shader_patch_synthetic_device COMMAND rsf_shader_patch_device_test)
include("${CMAKE_CURRENT_SOURCE_DIR}/d3d11_present_api.cmake")
add_executable(rsf_native_window_test native_window.cpp)
target_include_directories(rsf_native_window_test PRIVATE
    "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
target_link_libraries(rsf_native_window_test PRIVATE "$<TARGET_LINKER_FILE:rsf_orchestrator>" rsf_build_options)
add_dependencies(rsf_native_window_test rsf_orchestrator)
add_test(NAME native_window_contract COMMAND rsf_native_window_test)
include("${CMAKE_CURRENT_SOURCE_DIR}/d3d11_fg_api.cmake")
include("${CMAKE_CURRENT_SOURCE_DIR}/overlay_dll_api.cmake")
include("${CMAKE_CURRENT_SOURCE_DIR}/anomaly_support.cmake")
include("${CMAKE_CURRENT_SOURCE_DIR}/anomaly_fg.cmake")
include("${CMAKE_CURRENT_SOURCE_DIR}/thin_loader.cmake")
add_executable(rsf_anomaly_plugin_test anomaly_plugin.cpp)
target_link_libraries(rsf_anomaly_plugin_test PRIVATE ReScaleFrame::GameSDK rsf_build_options)
add_test(NAME anomaly_plugin_contract COMMAND rsf_anomaly_plugin_test
    "$<TARGET_FILE:rsf_game_anomaly>")
add_dependencies(rsf_anomaly_plugin_test rsf_game_anomaly)
add_executable(rsf_anomaly_lifecycle_test anomaly_lifecycle.cpp)
target_link_libraries(rsf_anomaly_lifecycle_test PRIVATE ReScaleFrame::GameSDK rsf_build_options)
add_dependencies(rsf_anomaly_lifecycle_test rsf_game_anomaly)
add_test(NAME anomaly_lifecycle_contract COMMAND rsf_anomaly_lifecycle_test "$<TARGET_FILE:rsf_game_anomaly>")
add_executable(rsf_anomaly_native_hooks_test anomaly_native_hooks.cpp ../games/anomaly/src/native_hooks.cpp)
target_include_directories(rsf_anomaly_native_hooks_test PRIVATE ../games/anomaly/src)
target_link_libraries(rsf_anomaly_native_hooks_test PRIVATE rsf_anomaly_rust minhook rsf_build_options)
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

rsf_add_rust_static(rsf_game_sdk_fixtures_rust rsf_rust_game_sdk_fixtures
    rsf-game-sdk-fixtures rsf_game_sdk_fixtures)
add_executable(rsf_plugin_contract game-sdk/contract_main.c)

add_executable(rsf_wingman_camera wingman_camera.c)
target_link_libraries(rsf_wingman_camera PRIVATE rsf_wingman_rust ReScaleFrame::GameSDK rsf_build_options)
add_test(NAME wingman_camera_contract COMMAND rsf_wingman_camera)
target_link_libraries(rsf_plugin_contract PRIVATE rsf_game_sdk_fixtures_rust ReScaleFrame::GameSDK rsf_build_options)
add_dependencies(rsf_plugin_contract rsf_game_ac7 rsf_game_project_wingman rsf_game_rdr2 rsf_game_unity_mono)
add_dependencies(rsf_plugin_contract rsf_game_skyrim_se)
add_dependencies(rsf_plugin_contract rsf_game_fallout4)
add_test(NAME game_fallout4_plugin_contract COMMAND rsf_plugin_contract
    "$<TARGET_FILE_NAME:rsf_game_fallout4>" fallout4)
set_tests_properties(game_fallout4_plugin_contract PROPERTIES
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_game_fallout4>")
add_executable(rsf_unity_sr_d3d12_test unity_sr_d3d12.cpp)
target_include_directories(rsf_unity_sr_d3d12_test PRIVATE ../games/unity-mono/include)
target_link_libraries(rsf_unity_sr_d3d12_test PRIVATE rsf_rdr2_rust rsf_orchestrator rsf_build_options minhook d3d12 dxgi)
if(EXISTS "${RSF_STREAMLINE_INCLUDE}/sl.h")
    target_include_directories(rsf_unity_sr_d3d12_test SYSTEM PRIVATE "${RSF_STREAMLINE_INCLUDE}")
endif()
add_test(NAME unity_sr_d3d12_contract COMMAND rsf_unity_sr_d3d12_test)
add_executable(rsf_render_frontend_test render_frontend.cpp)
target_link_libraries(rsf_render_frontend_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME render_frontend_contract COMMAND rsf_render_frontend_test)
add_executable(rsf_render_frontend_embedded_test render_frontend.cpp)
target_link_libraries(rsf_render_frontend_embedded_test PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options)
add_test(NAME render_frontend_embedded COMMAND rsf_render_frontend_embedded_test)
add_executable(rsf_callback_host_abi_test callback_host_abi.cpp)
target_link_libraries(rsf_callback_host_abi_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME callback_host_abi COMMAND rsf_callback_host_abi_test)
add_executable(rsf_callback_host_abi_embedded_test callback_host_abi.cpp)
target_link_libraries(rsf_callback_host_abi_embedded_test PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options)
add_test(NAME callback_host_abi_embedded COMMAND rsf_callback_host_abi_embedded_test)
set_tests_properties(callback_host_abi callback_host_abi_embedded PROPERTIES TIMEOUT 30)
add_executable(rsf_orchestrator_domains loader/main.c)
target_compile_definitions(rsf_orchestrator_domains PRIVATE RSF_FIXTURE_ENTRY=rsf_test_orchestrator_domains)
target_link_libraries(rsf_orchestrator_domains PRIVATE rsf_game_sdk_fixtures_rust rsf_build_options)
add_dependencies(rsf_orchestrator_domains rsf_orchestrator rsf_game_unity_mono)
add_test(NAME orchestrator_domain_contract COMMAND rsf_orchestrator_domains)
add_executable(rsf_d3d12_observer_test d3d12_observer.cpp)
target_link_libraries(rsf_d3d12_observer_test PRIVATE ReScaleFrame::Graphics rsf_build_options d3d12)
add_test(NAME d3d12_observer_contract COMMAND rsf_d3d12_observer_test)
add_executable(rsf_exposure_normalization_test exposure_normalization_main.c)
target_link_libraries(rsf_exposure_normalization_test PRIVATE rsf_orchestrator ReScaleFrame::Graphics rsf_build_options d3d12 dxgi)
add_test(NAME native_exposure_normalization COMMAND rsf_exposure_normalization_test)
add_executable(rsf_gpu_policy_test gpu_policy.cpp)
target_link_libraries(rsf_gpu_policy_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME gpu_sr_policy COMMAND rsf_gpu_policy_test)
add_executable(rsf_gpu_policy_embedded_test gpu_policy.cpp)
target_link_libraries(rsf_gpu_policy_embedded_test PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options)
add_test(NAME gpu_sr_policy_embedded COMMAND rsf_gpu_policy_embedded_test)
add_library(rsf_plugin_session_fixture SHARED plugin_session_fixture.cpp)
target_link_libraries(rsf_plugin_session_fixture PRIVATE ReScaleFrame::GameSDK rsf_build_options)
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
    rsf_add_loader_test(rsf_wingman_carrier_test rsf_test_xinput_carrier)
    add_dependencies(rsf_wingman_carrier_test rsf_proxy_wingman)
    add_test(NAME wingman_carrier_contract COMMAND rsf_wingman_carrier_test "$<TARGET_FILE:rsf_proxy_wingman>")
    set_tests_properties(wingman_carrier_contract PROPERTIES SKIP_RETURN_CODE 77)
    add_dependencies(rsf_wingman_carrier_test rsf_proxy_plugin)
    add_test(NAME plugin_carrier_contract COMMAND rsf_wingman_carrier_test "$<TARGET_FILE:rsf_proxy_plugin>")
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

add_executable(rsf_texture_dump texture_dump.cpp)
target_link_libraries(rsf_texture_dump PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME texture_dump COMMAND rsf_texture_dump "${CMAKE_CURRENT_BINARY_DIR}")

add_executable(rsf_input_dump_test input_dump.cpp)
add_library(rsf_input_dump_c_header OBJECT input_dump_c_header.c)
target_link_libraries(rsf_input_dump_c_header PRIVATE ReScaleFrame::FrameDump rsf_build_options)
target_link_libraries(rsf_input_dump_test PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options windowscodecs ole32)
add_test(NAME input_dump COMMAND rsf_input_dump_test "${CMAKE_CURRENT_BINARY_DIR}")
set_tests_properties(input_dump PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_input_dump_d3d12 input_dump_d3d12.cpp)
target_compile_definitions(rsf_input_dump_d3d12 PRIVATE RSF_FRAME_DUMP_STATIC)
target_link_libraries(rsf_input_dump_d3d12 PRIVATE ReScaleFrame::FrameDump rsf_build_options d3d12 dxgi)
add_test(NAME input_dump_d3d12 COMMAND rsf_input_dump_d3d12 "${CMAKE_CURRENT_BINARY_DIR}")
set_tests_properties(input_dump_d3d12 PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

if(RSF_FRAME_DUMP_VULKAN_INCLUDE)
    add_executable(rsf_input_dump_vulkan_cpu input_dump_vulkan_cpu.cpp)
    target_link_libraries(rsf_input_dump_vulkan_cpu PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME input_dump_vulkan_cpu_linked COMMAND rsf_input_dump_vulkan_cpu "${CMAKE_CURRENT_BINARY_DIR}")

    add_executable(rsf_input_dump_vulkan_cpu_dll input_dump_vulkan_cpu.cpp)
    target_include_directories(rsf_input_dump_vulkan_cpu_dll PRIVATE
        "$<TARGET_PROPERTY:rsf_frame_dump,INTERFACE_INCLUDE_DIRECTORIES>")
    target_include_directories(rsf_input_dump_vulkan_cpu_dll SYSTEM PRIVATE "${RSF_FRAME_DUMP_VULKAN_INCLUDE}")
    target_link_libraries(rsf_input_dump_vulkan_cpu_dll PRIVATE "$<TARGET_LINKER_FILE:rsf_orchestrator>"
        ReScaleFrame::GameSDK ReScaleFrame::Graphics ReScaleFrame::Contract rsf_build_options)
    add_dependencies(rsf_input_dump_vulkan_cpu_dll rsf_orchestrator)
    add_test(NAME input_dump_vulkan_cpu_dll COMMAND rsf_input_dump_vulkan_cpu_dll "${CMAKE_CURRENT_BINARY_DIR}")

    add_executable(rsf_input_dump_vulkan input_dump_vulkan.cpp)
    target_include_directories(rsf_input_dump_vulkan SYSTEM PRIVATE "${RSF_FRAME_DUMP_VULKAN_INCLUDE}")
    target_compile_definitions(rsf_input_dump_vulkan PRIVATE RSF_RUNTIME_BUILD)
    target_link_libraries(rsf_input_dump_vulkan PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME input_dump_vulkan COMMAND rsf_input_dump_vulkan "${CMAKE_CURRENT_BINARY_DIR}")
    set_tests_properties(input_dump_vulkan PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)
endif()

add_executable(rsf_frame_assembly frame_assembly.cpp)
target_link_libraries(rsf_frame_assembly PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME frame_assembly COMMAND rsf_frame_assembly)

add_executable(rsf_ac7_view_test ac7_view.cpp)
target_link_libraries(rsf_ac7_view_test PRIVATE ReScaleFrame::AC7View rsf_build_options)
add_test(NAME ac7_view COMMAND rsf_ac7_view_test)

add_executable(rsf_ac7_velocity_shader_test ac7_velocity_shader.cpp)
target_include_directories(rsf_ac7_velocity_shader_test PRIVATE "${CMAKE_SOURCE_DIR}/runtime/graphics/include")
target_link_libraries(rsf_ac7_velocity_shader_test PRIVATE ReScaleFrame::AC7RenderScope ReScaleFrame::AC7VelocityPass ReScaleFrame::RuntimeCommon rsf_build_options d3d11)
add_test(NAME ac7_velocity_shader COMMAND rsf_ac7_velocity_shader_test)
set_tests_properties(ac7_velocity_shader PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

rsf_add_rust_static(rsf_ac7_capture_fixture_rust rsf_rust_ac7_capture_fixture
    rsf-ac7-capture-fixture rsf_ac7_capture_fixture)
add_executable(rsf_ac7_capture_io ac7-capture/src/main.c)
target_link_libraries(rsf_ac7_capture_io PRIVATE rsf_ac7_capture_fixture_rust
    ReScaleFrame::AC7MotionCapture rsf_build_options d3dcompiler)
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
set(ac7_session_object "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>/rsf_ac7_session.obj")
add_custom_command(OUTPUT "${ac7_session_object}"
    COMMAND "${CMAKE_COMMAND}" -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>"
    COMMAND "${ac7_fixture_rustc}" --edition 2024 --target "${ac7_fixture_target}"
        --crate-name rsf_ac7_session --crate-type lib --emit obj -C codegen-units=1
        -C panic=abort -C opt-level=$<IF:$<CONFIG:Debug>,0,3>
        -o "${ac7_session_object}" "${CMAKE_CURRENT_SOURCE_DIR}/ac7_session.rs"
    DEPENDS ac7_session.rs "${PROJECT_SOURCE_DIR}/games/ac7/src/scene_color_abi.rs"
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
    VERBATIM)
set_source_files_properties("${ac7_session_object}" PROPERTIES GENERATED TRUE EXTERNAL_OBJECT TRUE)
add_executable(rsf_ac7_session ac7_session.c "${ac7_session_object}" ../loader/proxy/src/preferences.c)
set_target_properties(rsf_ac7_session PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_ac7_session PRIVATE ReScaleFrame::AC7SceneColor rsf_build_options)
add_test(NAME ac7_session COMMAND rsf_ac7_session)

include(g02/fixture.cmake)
rsf_add_g02_fixture(rsf_constant_override constant_override.rs)
target_link_libraries(rsf_constant_override PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options d3dcompiler)
add_test(NAME constant_override COMMAND rsf_constant_override)
set_tests_properties(constant_override PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

# No device: the classifier is a pure function over shadowed facts, which is the point of it.
add_executable(rsf_ac7_ui_rules_test ac7_ui_rules.cpp)
target_link_libraries(rsf_ac7_ui_rules_test PRIVATE ReScaleFrame::AC7UIRules rsf_build_options)
add_test(NAME ac7_ui_rules COMMAND rsf_ac7_ui_rules_test)

# A vendor that is not compiled in still has to answer, and has to answer differently from one
# whose runtime is missing. Runs in both configurations and CI builds it both ways.
add_executable(rsf_fsr_backend fsr_backend.c)
set_target_properties(rsf_fsr_backend PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_fsr_backend PRIVATE ReScaleFrame::FSR ReScaleFrame::XeSS rsf_build_options)
add_test(NAME fsr_backend COMMAND rsf_fsr_backend)

# Whether a D3D11 texture can be opened by a D3D12 device here at all. Skips with 77 and the
# HRESULTs when the two runtimes disagree, which under Wine is a fact about the environment.
rsf_add_rust_static(rsf_presentation_fixtures_rust rsf_rust_presentation_fixtures
    rsf-presentation-fixtures rsf_presentation_fixtures)
add_executable(rsf_shared_surface presentation/shared_surface_main.c)
set_target_properties(rsf_shared_surface PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON
    LINKER_LANGUAGE CXX)
target_link_libraries(rsf_shared_surface PRIVATE rsf_presentation_fixtures_rust
    ReScaleFrame::Presentation rsf_build_options)
add_test(NAME shared_surface COMMAND rsf_shared_surface)
set_tests_properties(shared_surface PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

# The layer, and the alpha accumulation that decides whether diverting into it works at all.
add_executable(rsf_ui_layer ui_layer.cpp)
target_link_libraries(rsf_ui_layer PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME ui_layer COMMAND rsf_ui_layer)
set_tests_properties(ui_layer PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

# The composite's arithmetic, which nothing covered while it lived in present_blit.
add_executable(rsf_fullscreen_pass graphics_fixture_main.c)
target_compile_definitions(rsf_fullscreen_pass PRIVATE RSF_GRAPHICS_FIXTURE=rsf_fullscreen_pass_fixture)
target_link_libraries(rsf_fullscreen_pass PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME fullscreen_pass COMMAND rsf_fullscreen_pass)
set_tests_properties(fullscreen_pass PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_native_reinsert graphics_fixture_main.c)
target_compile_definitions(rsf_native_reinsert PRIVATE RSF_GRAPHICS_FIXTURE=rsf_native_reinsert_fixture)
target_include_directories(rsf_native_reinsert PRIVATE ../runtime/orchestrator/include)
target_compile_definitions(rsf_native_reinsert PRIVATE RSF_RUNTIME_BUILD)
target_link_libraries(rsf_native_reinsert PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics ReScaleFrame::GameSDK rsf_build_options)
add_test(NAME native_reinsert_device COMMAND rsf_native_reinsert)
set_tests_properties(native_reinsert_device PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_depth_scale graphics_fixture_main.c)
target_compile_definitions(rsf_depth_scale PRIVATE RSF_GRAPHICS_FIXTURE=rsf_depth_scale_fixture)
target_link_libraries(rsf_depth_scale PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME depth_scale_device COMMAND rsf_depth_scale)
set_tests_properties(depth_scale_device PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_auxiliary_sr auxiliary_sr.cpp ../runtime/orchestrator/src/auxiliary_sr.cpp)
target_include_directories(rsf_auxiliary_sr PRIVATE ../runtime/orchestrator/include ../runtime/presentation/include)
target_link_libraries(rsf_auxiliary_sr PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics ReScaleFrame::GameSDK rsf_build_options)
add_test(NAME auxiliary_sr_device COMMAND rsf_auxiliary_sr)
set_tests_properties(auxiliary_sr_device PROPERTIES TIMEOUT 60 SKIP_RETURN_CODE 77)

add_executable(rsf_fallout4_input_state fallout4_input_state.cpp)
target_link_libraries(rsf_fallout4_input_state PRIVATE rsf_fallout4_rust rsf_build_options)
add_test(NAME fallout4_input_state_contract COMMAND rsf_fallout4_input_state)
add_executable(rsf_skyrim_input skyrim_input.cpp ../games/skyrim-se/src/cpu_events.cpp)
target_link_libraries(rsf_skyrim_input PRIVATE rsf_skyrim_se_rust ReScaleFrame::GameSDK minhook rsf_build_options)
add_test(NAME skyrim_input_contract COMMAND rsf_skyrim_input)

add_executable(rsf_native_cpu_reservation_test native_cpu_reservation.cpp)
target_compile_definitions(rsf_native_cpu_reservation_test PRIVATE RSF_RUNTIME_BUILD)
target_link_libraries(rsf_native_cpu_reservation_test PRIVATE ReScaleFrame::GameSDK rsf_native_cpu rsf_build_options)
add_test(NAME native_cpu_reservation_contract COMMAND rsf_native_cpu_reservation_test)

add_executable(rsf_native_fg_cpu_test native_fg_cpu.cpp)
target_include_directories(rsf_native_fg_cpu_test PRIVATE "${PROJECT_SOURCE_DIR}/runtime/orchestrator/src")
target_compile_definitions(rsf_native_fg_cpu_test PRIVATE RSF_RUNTIME_BUILD)
target_link_libraries(rsf_native_fg_cpu_test PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options)
add_test(NAME native_fg_cpu_contract COMMAND rsf_native_fg_cpu_test)
set_tests_properties(native_fg_cpu_contract PROPERTIES TIMEOUT 15)

add_executable(rsf_sampler_bias graphics_fixture_main.c)
target_compile_definitions(rsf_sampler_bias PRIVATE RSF_GRAPHICS_FIXTURE=rsf_sampler_bias_fixture)
target_include_directories(rsf_sampler_bias PRIVATE ../runtime/orchestrator/include ../runtime/presentation/include)
target_link_libraries(rsf_sampler_bias PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics ReScaleFrame::GameSDK rsf_build_options)
add_test(NAME sampler_bias_device COMMAND rsf_sampler_bias)
set_tests_properties(sampler_bias_device PROPERTIES TIMEOUT 60 SKIP_RETURN_CODE 77)

add_executable(rsf_d3d11_host_test d3d11_host.cpp d3d11_host_c_header.c)
target_include_directories(rsf_d3d11_host_test PRIVATE "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
target_link_libraries(rsf_d3d11_host_test PRIVATE "$<TARGET_LINKER_FILE:rsf_orchestrator>" rsf_build_options)
add_dependencies(rsf_d3d11_host_test rsf_orchestrator)
add_test(NAME d3d11_host_contract COMMAND rsf_d3d11_host_test)

add_executable(rsf_resource_roles graphics_fixture_main.c)
target_compile_definitions(rsf_resource_roles PRIVATE RSF_GRAPHICS_FIXTURE=rsf_resource_roles_fixture)
target_link_libraries(rsf_resource_roles PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options
add_test(NAME resource_roles COMMAND rsf_resource_roles)

add_executable(rsf_motion_decode graphics_fixture_main.c)
target_compile_definitions(rsf_motion_decode PRIVATE RSF_GRAPHICS_FIXTURE=rsf_motion_decode_fixture)
target_link_libraries(rsf_motion_decode PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME motion_decode COMMAND rsf_motion_decode)
set_tests_properties(motion_decode PROPERTIES TIMEOUT 120)

add_executable(rsf_frame_tap frame_tap.c)
set_target_properties(rsf_frame_tap PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_frame_tap PRIVATE ReScaleFrame::AC7SceneColor ReScaleFrame::Graphics rsf_build_options)
add_test(NAME frame_tap COMMAND rsf_frame_tap)
set_tests_properties(frame_tap PROPERTIES TIMEOUT 120)

add_executable(rsf_d3d11_state d3d11_state.cpp)
target_link_libraries(rsf_d3d11_state PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME d3d11_state COMMAND rsf_d3d11_state)
set_tests_properties(d3d11_state PROPERTIES TIMEOUT 120)

rsf_add_g02_fixture(rsf_scene_promote scene_promote.rs)
target_link_libraries(rsf_scene_promote PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options)
add_test(NAME scene_promote COMMAND rsf_scene_promote)
set_tests_properties(scene_promote PROPERTIES TIMEOUT 120)

add_executable(rsf_d3d11_observer d3d11_observer.cpp)
target_link_libraries(rsf_d3d11_observer PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME d3d11_observer COMMAND rsf_d3d11_observer "${CMAKE_CURRENT_BINARY_DIR}")
# These touch a graphics runtime and patch vtables, so a deadlock is a plausible failure. Fail in
# a couple of minutes rather than hanging the suite.
set_tests_properties(d3d11_observer texture_dump PROPERTIES TIMEOUT 120)

add_library(rsf_overlay_panel_fixture SHARED overlay_panel_fixture.cpp)
target_link_libraries(rsf_overlay_panel_fixture PRIVATE ReScaleFrame::Graphics rsf_build_options)
# Exported by property rather than by dllexport on each definition: the header declares the
# functions plainly, and MSVC refuses a definition whose linkage differs from its declaration.
set_target_properties(rsf_overlay_panel_fixture PROPERTIES PREFIX "" WINDOWS_EXPORT_ALL_SYMBOLS ON)
rsf_add_rust_static(rsf_overlay_fixtures_rust rsf_rust_overlay_fixtures
    rsf-overlay-fixtures rsf_overlay_fixtures)
add_executable(rsf_overlay_host overlay/main.c)
target_compile_definitions(rsf_overlay_host PRIVATE RSF_OVERLAY_FIXTURE=rsf_test_overlay_host)
target_link_libraries(rsf_overlay_host PRIVATE rsf_overlay_fixtures_rust rsf_orchestrator_rust
    ReScaleFrame::Graphics rsf_build_options)

add_executable(rsf_overlay_hotkey overlay/main.c)
target_compile_definitions(rsf_overlay_hotkey PRIVATE RSF_OVERLAY_FIXTURE=rsf_test_overlay_hotkey)
target_link_libraries(rsf_overlay_hotkey PRIVATE rsf_overlay_fixtures_rust rsf_orchestrator_rust
    ReScaleFrame::Graphics rsf_build_options)
add_test(NAME overlay_hotkey COMMAND rsf_overlay_hotkey)
set_tests_properties(overlay_hotkey PROPERTIES TIMEOUT 30)
add_dependencies(rsf_overlay_host rsf_overlay_panel_fixture)
add_test(NAME overlay_host COMMAND rsf_overlay_host "$<TARGET_FILE_NAME:rsf_overlay_panel_fixture>")
set_tests_properties(overlay_host PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_overlay_host>")
add_executable(rsf_overlay_native12_test overlay_native12.cpp ../runtime/orchestrator/src/overlay_native12.cpp)
target_include_directories(rsf_overlay_native12_test PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_overlay_native12_test PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options d3d12 d3dcompiler dxgi)
add_dependencies(rsf_overlay_native12_test rsf_overlay_panel_fixture)
add_test(NAME overlay_native12_backpressure COMMAND rsf_overlay_native12_test "$<TARGET_FILE:rsf_overlay_panel_fixture>")
set_tests_properties(overlay_native12_backpressure PROPERTIES TIMEOUT 30 SKIP_RETURN_CODE 77)

add_executable(rsf_dlss_backend dlss_backend.cpp)
target_link_libraries(rsf_dlss_backend PRIVATE ReScaleFrame::DLSS rsf_build_options)
add_test(NAME dlss_backend COMMAND rsf_dlss_backend)
# Loading a vendor runtime that talks to a driver is another place a hang is plausible.
set_tests_properties(dlss_backend PROPERTIES TIMEOUT 120)

add_test(NAME launcher_version COMMAND rsf_launcher --version)
set_tests_properties(launcher_version PROPERTIES PASS_REGULAR_EXPRESSION "ReScaleFrame ${PROJECT_VERSION}")

add_executable(rsf_sr_bridge_test sr_bridge.cpp)
target_link_libraries(rsf_sr_bridge_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME sr_bridge COMMAND rsf_sr_bridge_test)
set_tests_properties(sr_bridge PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)

# Link controller against deterministic providers, with no vendor DLLs or graphics device.
add_executable(rsf_sr_session_test sr_session.cpp ../runtime/orchestrator/src/sr_session_backends.cpp)
target_compile_definitions(rsf_sr_session_test PRIVATE RSF_RUNTIME_BUILD)
target_include_directories(rsf_sr_session_test PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_sr_session_test PRIVATE rsf_orchestrator_rust ReScaleFrame::Contract ReScaleFrame::RuntimeCommon rsf_build_options)
add_test(NAME sr_session COMMAND rsf_sr_session_test)

add_executable(rsf_motion_resolve_test graphics_fixture_main.c)
target_compile_definitions(rsf_motion_resolve_test PRIVATE RSF_GRAPHICS_FIXTURE=rsf_motion_resolve_fixture)
target_link_libraries(rsf_motion_resolve_test PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME motion_resolve COMMAND rsf_motion_resolve_test)
set_tests_properties(motion_resolve PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)

add_executable(rsf_colour_transport_test graphics_fixture_main.c)
target_compile_definitions(rsf_colour_transport_test PRIVATE RSF_GRAPHICS_FIXTURE=rsf_colour_transport_fixture)
target_link_libraries(rsf_colour_transport_test PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME colour_transport COMMAND rsf_colour_transport_test)
set_tests_properties(colour_transport PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)
add_executable(rsf_native_translucency_test native_translucency.cpp)
target_link_libraries(rsf_native_translucency_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME native_translucency COMMAND rsf_native_translucency_test)
set_tests_properties(native_translucency PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)
add_executable(rsf_native_translucency_abi_test native_translucency_abi.cpp)
target_link_libraries(rsf_native_translucency_abi_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME native_translucency_abi COMMAND rsf_native_translucency_abi_test)
add_executable(rsf_native_translucency_abi_embedded_test native_translucency_abi.cpp)
target_link_libraries(rsf_native_translucency_abi_embedded_test PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options)
add_test(NAME native_translucency_abi_embedded COMMAND rsf_native_translucency_abi_embedded_test)
add_executable(rsf_sr_pipeline_test sr_pipeline.cpp)
target_link_libraries(rsf_sr_pipeline_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME sr_pipeline COMMAND rsf_sr_pipeline_test)
set_tests_properties(sr_pipeline PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)

add_executable(rsf_fg_session_test fg_session.cpp)
target_link_libraries(rsf_fg_session_test PRIVATE rsf_orchestrator rsf_build_options)
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

add_executable(rsf_fg_provider_test fg_provider.cpp)
add_executable(rsf_fg_choice_test fg_choice.cpp)
target_link_libraries(rsf_fg_choice_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME fg_choice COMMAND rsf_fg_choice_test)
target_link_libraries(rsf_fg_provider_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME fg_provider COMMAND rsf_fg_provider_test)
set_tests_properties(fg_provider PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_fg_leases_test fg_leases.cpp)
target_link_libraries(rsf_fg_leases_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME fg_leases COMMAND rsf_fg_leases_test)
set_tests_properties(fg_leases PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_fg_leases_faults_test fg_leases_faults.cpp)
target_link_libraries(rsf_fg_leases_faults_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME fg_leases_faults COMMAND rsf_fg_leases_faults_test)

add_library(rsf_fg_c_header OBJECT fg_c_header.c)
target_include_directories(rsf_fg_c_header PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_fg_c_header PRIVATE ReScaleFrame::DLSS rsf_build_options)

add_executable(rsf_fg_switching_test fg_switching.cpp)
add_executable(rsf_d3d11_present_bridge_test d3d11_present_bridge.cpp)
add_executable(rsf_fg_present_bridge_test fg_present_bridge.cpp)
target_link_libraries(rsf_fg_present_bridge_test PRIVATE rsf_orchestrator_core rsf_build_options)
set_target_properties(rsf_fg_present_bridge_test PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/fixtures/fg/$<CONFIG>")
add_test(NAME shared_fg_present_bridge COMMAND rsf_fg_present_bridge_test)
set_tests_properties(shared_fg_present_bridge PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)
target_compile_definitions(rsf_d3d11_present_bridge_test PRIVATE RSF_STREAMLINE_HOST_BUILD)
# The render scope calls other than create are exported by the rsf-ac7 crate (render_scope.rs).
target_link_libraries(rsf_d3d11_present_bridge_test PRIVATE rsf_orchestrator_core ReScaleFrame::AC7RenderScope rsf_ac7_rust rsf_build_options)
# Graphics fixtures must not load game-facing proxy DLLs from the shared artifact directory.
set_target_properties(rsf_d3d11_present_bridge_test PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/fixtures/fg/$<CONFIG>")
add_test(NAME d3d11_present_bridge COMMAND rsf_d3d11_present_bridge_test)
set_tests_properties(d3d11_present_bridge PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)
add_dependencies(rsf_d3d11_present_bridge_test rsf_proxy_ac7)
add_test(NAME ac7_proxy_startup COMMAND rsf_d3d11_present_bridge_test
    --proxy-smoke $<TARGET_FILE:rsf_proxy_ac7>)
set_tests_properties(ac7_proxy_startup PROPERTIES TIMEOUT 120)
target_link_libraries(rsf_fg_switching_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME fg_switching COMMAND rsf_fg_switching_test)
set_tests_properties(fg_switching PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_latency_host_test latency_host.cpp)

# These latency fixtures supply fake FG endpoints while linking the production Rust lease.
set(rsf_vulkan_latency_test_endpoints
    rsf_fgvk_backend=rsf_test_fgvk_backend
    rsf_fgvk_latency_options=rsf_test_fgvk_latency_options
    rsf_fgvk_latency_sleep=rsf_test_fgvk_latency_sleep
    rsf_fgvk_latency_marker=rsf_test_fgvk_latency_marker
    rsf_fgvk_latency_get_status=rsf_test_fgvk_latency_get_status)
add_executable(rsf_native_latency_vulkan_test native_latency_vulkan.cpp ../runtime/orchestrator/src/native_latency_vulkan.cpp)
target_sources(rsf_native_latency_vulkan_test PRIVATE ../runtime/orchestrator/src/latency_amd_vulkan.cpp)
target_include_directories(rsf_native_latency_vulkan_test PRIVATE ../runtime/orchestrator/include)
target_compile_definitions(rsf_native_latency_vulkan_test PRIVATE RSF_RUNTIME_BUILD RSF_HAVE_STREAMLINE_VK=1
    ${rsf_vulkan_latency_test_endpoints})
target_link_libraries(rsf_native_latency_vulkan_test PRIVATE ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_native_cpu rsf_build_options)
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
find_program(RSF_RUSTC rustc REQUIRED)
if(MSVC)
    set(godot_graph_rust_target x86_64-pc-windows-msvc)
else()
    set(godot_graph_rust_target x86_64-pc-windows-gnu)
endif()
set(godot_module "${PROJECT_SOURCE_DIR}/integrations/godot/modules/rescaleframe")
set(godot_graph_object "${CMAKE_CURRENT_BINARY_DIR}/godot_graph_module${CMAKE_CXX_OUTPUT_EXTENSION}")
add_custom_command(OUTPUT "${godot_graph_object}"
    COMMAND "${RSF_RUSTC}" --edition=2024 --crate-type=lib --emit=obj --target ${godot_graph_rust_target}
        -Cpanic=abort -Copt-level=2 -Ccodegen-units=1 -Dwarnings "${godot_module}/viewport.rs" -o "${godot_graph_object}"
    DEPENDS "${godot_module}/viewport.rs" "${godot_module}/cpu_facts.rs" "${godot_module}/frame.rs"
        "${godot_module}/graph.rs" "${godot_module}/host.rs" "${godot_module}/pass.rs"
        "${godot_module}/present.rs" "${godot_module}/primary.rs"
        "${PROJECT_SOURCE_DIR}/sdk/game/src/frame_event_abi.rs" "${PROJECT_SOURCE_DIR}/rust-toolchain.toml"
        "${PROJECT_SOURCE_DIR}/sdk/game/src/frame_abi.rs"
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
    VERBATIM)
set_source_files_properties("${godot_graph_object}" PROPERTIES EXTERNAL_OBJECT TRUE GENERATED TRUE)
add_executable(rsf_godot_graph_handoff_test godot_graph_handoff.cpp "${godot_graph_object}")
target_include_directories(rsf_godot_graph_handoff_test PRIVATE godot_graph_stubs ../integrations/godot/native ../runtime/orchestrator/include)
target_link_libraries(rsf_godot_graph_handoff_test PRIVATE ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_build_options)
add_test(NAME godot_graph_handoff_contract COMMAND rsf_godot_graph_handoff_test)
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h" AND EXISTS "${RSF_FFX_INCLUDE}/dx12/ffx_api_dx12.h" AND EXISTS "${RSF_FFX_INCLUDE}/../../upscalers/include/ffx_upscale.h")
    add_executable(rsf_native_sr_fsr4_vulkan_test native_sr_fsr4_vulkan.cpp)
    target_include_directories(rsf_native_sr_fsr4_vulkan_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_native_sr_fsr4_vulkan_test PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME native_sr_fsr4_vulkan_contract COMMAND rsf_native_sr_fsr4_vulkan_test)
    add_executable(rsf_native_sr_fsr4_state_test native_sr_fsr4_state.cpp)
    target_include_directories(rsf_native_sr_fsr4_state_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_native_sr_fsr4_state_test PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME native_sr_fsr4_state_contract COMMAND rsf_native_sr_fsr4_state_test)
endif()
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h" AND EXISTS "${RSF_XESS_VK_INCLUDE}/xess_fg/xefg_swapchain_d3d12.h" AND EXISTS "${RSF_XESS_VK_INCLUDE}/xell/xell_d3d12.h")
    add_executable(rsf_native_fg_xess_vulkan_test native_fg_xess_vulkan_interop.cpp native_fg_xess_vulkan_present.cpp)
    target_include_directories(rsf_native_fg_xess_vulkan_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_native_fg_xess_vulkan_test PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME native_fg_xess_vulkan_contract COMMAND rsf_native_fg_xess_vulkan_test)
endif()
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h")
    add_executable(rsf_ffx_vulkan_wsi_dispatch_test ffx_vulkan_wsi_dispatch.cpp)
    target_include_directories(rsf_ffx_vulkan_wsi_dispatch_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_ffx_vulkan_wsi_dispatch_test PRIVATE rsf_build_options)
    add_test(NAME ffx_vulkan_wsi_dispatch COMMAND rsf_ffx_vulkan_wsi_dispatch_test)
    if(EXISTS "${RSF_FFX_VK_INCLUDE}/ffx_api/vk/ffx_api_vk.h")
        add_executable(rsf_native_ffx_vulkan_reset_test native_ffx_vulkan_reset.cpp)
        target_include_directories(rsf_native_ffx_vulkan_reset_test PRIVATE ../runtime/orchestrator/include)
        target_include_directories(rsf_native_ffx_vulkan_reset_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_FFX_VK_INCLUDE}")
        target_compile_definitions(rsf_native_ffx_vulkan_reset_test PRIVATE RSF_RUNTIME_BUILD RSF_HAVE_FFX_VK=1)
        target_link_libraries(rsf_native_ffx_vulkan_reset_test PRIVATE rsf_orchestrator_rust ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::FrameDump rsf_build_options)
        add_test(NAME native_ffx_vulkan_reset_contract COMMAND rsf_native_ffx_vulkan_reset_test)
        add_executable(rsf_native_ffx_vulkan_submission_test native_ffx_vulkan_submission.cpp)
        target_include_directories(rsf_native_ffx_vulkan_submission_test PRIVATE ../runtime/orchestrator/include)
        target_include_directories(rsf_native_ffx_vulkan_submission_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_FFX_VK_INCLUDE}")
        target_compile_definitions(rsf_native_ffx_vulkan_submission_test PRIVATE RSF_RUNTIME_BUILD RSF_HAVE_FFX_VK=1)
        target_link_libraries(rsf_native_ffx_vulkan_submission_test PRIVATE rsf_orchestrator_rust ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::FrameDump rsf_build_options)
        add_test(NAME native_ffx_vulkan_submission_contract COMMAND rsf_native_ffx_vulkan_submission_test)
        add_executable(rsf_native_ffx_vulkan_wsi_route_test native_ffx_vulkan_wsi_route.cpp)
        target_include_directories(rsf_native_ffx_vulkan_wsi_route_test PRIVATE ../runtime/orchestrator/include)
        target_include_directories(rsf_native_ffx_vulkan_wsi_route_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_FFX_VK_INCLUDE}")
        target_compile_definitions(rsf_native_ffx_vulkan_wsi_route_test PRIVATE RSF_RUNTIME_BUILD RSF_HAVE_FFX_VK=1)
        target_link_libraries(rsf_native_ffx_vulkan_wsi_route_test PRIVATE rsf_orchestrator_rust ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::FrameDump rsf_build_options)
        add_test(NAME native_ffx_vulkan_wsi_route_contract COMMAND rsf_native_ffx_vulkan_wsi_route_test)
    endif()
endif()
target_link_libraries(rsf_latency_host_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME latency_host COMMAND rsf_latency_host_test)
set_tests_properties(latency_host PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_render_links_test render_links.cpp)
target_link_libraries(rsf_render_links_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME render_links COMMAND rsf_render_links_test)
set_tests_properties(render_links PROPERTIES TIMEOUT 120)

add_executable(rsf_ac7_render_scope_test ac7_render_scope.cpp)
# render_scope.cpp keeps create; the rsf-ac7 crate owns and exports the rest (render_scope.rs).
target_link_libraries(rsf_ac7_render_scope_test PRIVATE ReScaleFrame::AC7RenderScope rsf_ac7_rust rsf_build_options)
add_test(NAME ac7_render_scope COMMAND rsf_ac7_render_scope_test)
set_tests_properties(ac7_render_scope PROPERTIES TIMEOUT 120)

# Explicit hardware experiment, never run by routine CTest verification.
rsf_add_rust_static(rsf_exposure_probe_rust rsf_exposure_probe_rust_build rsf-exposure-probe rsf_exposure_probe)
add_executable(rsf_exposure_probe ../tools/exposure-probe.cpp)
target_link_libraries(rsf_exposure_probe PRIVATE rsf_exposure_probe_rust ReScaleFrame::DLSS ReScaleFrame::Graphics minhook rsf_build_options)
if(EXISTS "${RSF_RENDERDOC_INCLUDE}/renderdoc_app.h")
    target_include_directories(rsf_exposure_probe SYSTEM PRIVATE "${RSF_RENDERDOC_INCLUDE}")
    target_compile_definitions(rsf_exposure_probe PRIVATE RSF_PROBE_RENDERDOC=1)
endif()
set(RSF_NGX_PROBE_INCLUDE "" CACHE PATH "Optional NGX SDK headers for standalone probe parameter tracing")
if(EXISTS "${RSF_NGX_PROBE_INCLUDE}/nvsdk_ngx_params.h")
    target_include_directories(rsf_exposure_probe SYSTEM PRIVATE "${RSF_NGX_PROBE_INCLUDE}")
    target_compile_definitions(rsf_exposure_probe PRIVATE RSF_PROBE_NGX_PARAMS=1)
endif()

add_executable(rsf_ac7_native_graph_test ac7_native_graph.cpp)
# native_renderer.cpp calls the rsf-ac7 site, patch, guard, renderer transaction, TrueSky and render
# scope owners (games/ac7/src/ac7_sites.h, ac7_render_scope.h).
target_link_libraries(rsf_ac7_native_graph_test PRIVATE ReScaleFrame::AC7RenderScope rsf_ac7_rust rsf_orchestrator minhook rsf_build_options)
add_test(NAME ac7_native_graph COMMAND rsf_ac7_native_graph_test)
set_tests_properties(ac7_native_graph PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_gl_shared_surface_test gl_shared_surface.cpp)
target_link_libraries(rsf_gl_shared_surface_test PRIVATE rsf_minecraft_rust ReScaleFrame::OrchestratorCore rsf_build_options opengl32)
add_test(NAME gl_shared_surface COMMAND rsf_gl_shared_surface_test)
set_tests_properties(gl_shared_surface PROPERTIES TIMEOUT 60 SKIP_RETURN_CODE 77)

add_executable(rsf_native_vulkan_test native_sr_vulkan.cpp)
target_link_libraries(rsf_native_vulkan_test PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h" AND EXISTS "${RSF_FFX_VK_INCLUDE}/ffx_api/vk/ffx_api_vk.h")
    target_compile_definitions(rsf_native_vulkan_test PRIVATE RSF_TEST_VULKAN=1)
    target_include_directories(rsf_native_vulkan_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
endif()
add_test(NAME native_vulkan_contract COMMAND rsf_native_vulkan_test)
add_library(rsf_native_split_c_header OBJECT native_sr_split_vulkan.c)
target_include_directories(rsf_native_split_c_header PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_native_split_c_header PRIVATE ReScaleFrame::Contract ReScaleFrame::GameSDK rsf_build_options)
add_test(NAME native_vulkan_device COMMAND rsf_native_vulkan_test --device)
add_test(NAME native_vulkan_dlss_device COMMAND rsf_native_vulkan_test --dlss)
set_tests_properties(native_vulkan_device native_vulkan_dlss_device PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_native_xess_vulkan_test native_xess_vulkan.cpp)
target_link_libraries(rsf_native_xess_vulkan_test PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h" AND EXISTS "${RSF_XESS_VK_INCLUDE}/xess/xess_vk.h")
    target_compile_definitions(rsf_native_xess_vulkan_test PRIVATE RSF_TEST_VULKAN=1)
    target_include_directories(rsf_native_xess_vulkan_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
endif()
add_test(NAME native_xess_vulkan_contract COMMAND rsf_native_xess_vulkan_test)
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h" AND EXISTS "${RSF_XESS_VK_INCLUDE}/xess/xess_vk.h" AND EXISTS "${RSF_FFX_VK_INCLUDE}/ffx_api/vk/ffx_api_vk.h")
    add_executable(rsf_native_xess_input_range_test native_xess_vulkan.cpp)
    target_compile_definitions(rsf_native_xess_input_range_test PRIVATE RSF_TEST_XESS_INPUT_RANGE RSF_HAVE_XESS_VK RSF_RUNTIME_BUILD)
    target_include_directories(rsf_native_xess_input_range_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_XESS_VK_INCLUDE}")
    target_link_libraries(rsf_native_xess_input_range_test PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME native_xess_input_range_contract COMMAND rsf_native_xess_input_range_test)
endif()
add_executable(rsf_native_fg_vulkan_dispatch_test native_fg_vulkan_dispatch.cpp)
target_compile_definitions(rsf_native_fg_vulkan_dispatch_test PRIVATE RSF_RUNTIME_BUILD)
target_include_directories(rsf_native_fg_vulkan_dispatch_test PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_native_fg_vulkan_dispatch_test PRIVATE rsf_orchestrator_rust ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_build_options)
add_test(NAME native_fg_vulkan_dispatch_contract COMMAND rsf_native_fg_vulkan_dispatch_test)
add_executable(rsf_native_fg_vulkan_dispatch_xess_test native_fg_vulkan_dispatch.cpp)
target_compile_definitions(rsf_native_fg_vulkan_dispatch_xess_test PRIVATE RSF_RUNTIME_BUILD RSF_HAVE_XESS_FG_VK=1)
target_include_directories(rsf_native_fg_vulkan_dispatch_xess_test PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_native_fg_vulkan_dispatch_xess_test PRIVATE rsf_orchestrator_rust ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_build_options)
add_test(NAME native_fg_vulkan_dispatch_xess_contract COMMAND rsf_native_fg_vulkan_dispatch_xess_test)
add_executable(rsf_native_fg_vulkan_dispatch_fsr4_test native_fg_vulkan_dispatch.cpp)
target_compile_definitions(rsf_native_fg_vulkan_dispatch_fsr4_test PRIVATE RSF_RUNTIME_BUILD RSF_HAVE_FSR4_FG_VK=1)
target_include_directories(rsf_native_fg_vulkan_dispatch_fsr4_test PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_native_fg_vulkan_dispatch_fsr4_test PRIVATE rsf_orchestrator_rust ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_build_options)
add_test(NAME native_fg_vulkan_dispatch_fsr4_contract COMMAND rsf_native_fg_vulkan_dispatch_fsr4_test)
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h" AND EXISTS "${RSF_FFX_INCLUDE}/dx12/ffx_api_dx12.h" AND EXISTS "${RSF_FFX_INCLUDE}/../../framegeneration/include/ffx_framegeneration.h")
    add_executable(rsf_native_fsr4_fg_bridge_test native_fsr4_fg_bridge.cpp)
    target_link_libraries(rsf_native_fsr4_fg_bridge_test PRIVATE ReScaleFrame::NativeVulkan rsf_build_options dxgi)
    add_test(NAME native_fsr4_fg_bridge_contract COMMAND rsf_native_fsr4_fg_bridge_test)
    add_executable(rsf_native_fsr4_fg_state_test native_fsr4_fg_state.cpp)
    target_include_directories(rsf_native_fsr4_fg_state_test PRIVATE ../runtime/orchestrator/include)
    target_include_directories(rsf_native_fsr4_fg_state_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_compile_definitions(rsf_native_fsr4_fg_state_test PRIVATE RSF_RUNTIME_BUILD)
    target_link_libraries(rsf_native_fsr4_fg_state_test PRIVATE ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_orchestrator_rust rsf_build_options)
    add_test(NAME native_fsr4_fg_state_contract COMMAND rsf_native_fsr4_fg_state_test)
endif()
add_test(NAME native_xess_vulkan_device COMMAND rsf_native_xess_vulkan_test --device)
set_tests_properties(native_xess_vulkan_device PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h")
    add_executable(rsf_streamline_vulkan_dispatch_test streamline_vulkan_dispatch.cpp)
    target_include_directories(rsf_streamline_vulkan_dispatch_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_streamline_vulkan_dispatch_test PRIVATE rsf_build_options)
    add_test(NAME streamline_vulkan_dispatch_contract COMMAND rsf_streamline_vulkan_dispatch_test)
    add_executable(rsf_native_antilag_vulkan_test native_antilag_vulkan.cpp
        ../runtime/orchestrator/src/native_latency_vulkan.cpp ../runtime/orchestrator/src/latency_amd_vulkan.cpp)
    target_compile_definitions(rsf_native_antilag_vulkan_test PRIVATE RSF_RUNTIME_BUILD RSF_HAVE_ANTILAG_VK=1
        ${rsf_vulkan_latency_test_endpoints})
    target_include_directories(rsf_native_antilag_vulkan_test PRIVATE ../runtime/orchestrator/include)
    target_include_directories(rsf_native_antilag_vulkan_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_native_antilag_vulkan_test PRIVATE ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_native_cpu rsf_build_options)
    add_test(NAME native_antilag_vulkan_contract COMMAND rsf_native_antilag_vulkan_test)
    set_tests_properties(native_antilag_vulkan_contract PROPERTIES TIMEOUT 20)
endif()

if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h" AND EXISTS "${RSF_STREAMLINE_INCLUDE}/sl.h")
    add_executable(rsf_streamline_selected_device_test streamline_selected_device.cpp)
    get_filename_component(rsf_streamline_source_root "${RSF_STREAMLINE_INCLUDE}" DIRECTORY)
    target_include_directories(rsf_streamline_selected_device_test SYSTEM PRIVATE "${rsf_streamline_source_root}")
    target_link_libraries(rsf_streamline_selected_device_test PRIVATE rsf_build_options)
    add_test(NAME streamline_selected_device_contract COMMAND rsf_streamline_selected_device_test)
    add_executable(rsf_native_fg_dlss_vulkan_test native_fg_dlss_vulkan.cpp)
    target_compile_definitions(rsf_native_fg_dlss_vulkan_test PRIVATE RSF_RUNTIME_BUILD RSF_HAVE_STREAMLINE_VK=1)
    target_include_directories(rsf_native_fg_dlss_vulkan_test PRIVATE ../runtime/orchestrator/include)
    target_include_directories(rsf_native_fg_dlss_vulkan_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_STREAMLINE_INCLUDE}")
    target_link_libraries(rsf_native_fg_dlss_vulkan_test PRIVATE rsf_orchestrator_rust ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::FrameDump rsf_build_options)
    add_test(NAME native_fg_dlss_vulkan_contract COMMAND rsf_native_fg_dlss_vulkan_test)
    add_executable(rsf_native_dlss_vulkan_test native_dlss_vulkan.cpp)
    target_include_directories(rsf_native_dlss_vulkan_test PRIVATE ../runtime/orchestrator/include)
    target_include_directories(rsf_native_dlss_vulkan_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_STREAMLINE_INCLUDE}")
    target_compile_definitions(rsf_native_dlss_vulkan_test PRIVATE RSF_RUNTIME_BUILD RSF_HAVE_STREAMLINE_VK=1)
    target_link_libraries(rsf_native_dlss_vulkan_test PRIVATE rsf_orchestrator_rust ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::FrameDump rsf_build_options)
    add_test(NAME native_dlss_vulkan_contract COMMAND rsf_native_dlss_vulkan_test)
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

    add_executable(rsf_vendor_census_test vendor_census.cpp)
    target_link_libraries(rsf_vendor_census_test PRIVATE ReScaleFrame::VendorCensus rsf_census_fx_fsr2 rsf_build_options)
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
        # The camera decode is owned by the Rust crate rsf-rdr2; the fixture calls its C ABI.
        add_executable(rsf_rdr2_camera rdr2_camera.cpp)
        target_link_libraries(rsf_rdr2_camera PRIVATE rsf_rdr2_rust ReScaleFrame::GameSDK minhook user32 rsf_build_options)
        add_test(NAME rdr2_camera COMMAND rsf_rdr2_camera "${CMAKE_CURRENT_SOURCE_DIR}/fixtures/rdr2-camera-v1.bin")
        add_executable(rsf_rdr2_capture rdr2_capture.cpp ../games/rdr2/src/native_capture.cpp)
        target_compile_definitions(rsf_rdr2_capture PRIVATE RSF_RDR2_CAPTURE_TEST _CRT_SECURE_NO_WARNINGS)
        target_link_libraries(rsf_rdr2_capture PRIVATE rsf_rdr2_rust ReScaleFrame::VendorCensus minhook d3d12 dxgi d3dcompiler rsf_build_options)
        add_dependencies(rsf_rdr2_capture rsf_census_fx_ngx)
        add_test(NAME rdr2_capture COMMAND rsf_rdr2_capture "${CMAKE_CURRENT_BINARY_DIR}/rdr2-capture")
        add_test(NAME rdr2_capture_rgb10 COMMAND rsf_rdr2_capture "${CMAKE_CURRENT_BINARY_DIR}/rdr2-capture-rgb10" rgb10)
        set_tests_properties(rdr2_capture rdr2_capture_rgb10 PROPERTIES TIMEOUT 60)
    endif()
    # Hardware and desktop reproduction of the census next to an injected overlay; run by hand.
    add_executable(rsf_census_present_coexistence loader/census_present_main.c)
    target_link_libraries(rsf_census_present_coexistence PRIVATE
        rsf_loader_fixtures_rust ReScaleFrame::VendorCensus d3d12 dxgi user32 rsf_build_options)

    # The census restates vendor structure layouts so it builds without the vendor headers. These
    # tests compare the restated offsets with the headers where an untracked checkout provides them.
    function(rsf_census_layout_check suffix define probe)
        if(EXISTS "${probe}")
            add_executable(rsf_vendor_census_layouts_${suffix} vendor_census_layouts.cpp)
            target_compile_definitions(rsf_vendor_census_layouts_${suffix} PRIVATE ${define})
            target_include_directories(rsf_vendor_census_layouts_${suffix} SYSTEM PRIVATE ${ARGN})
            target_link_libraries(rsf_vendor_census_layouts_${suffix} PRIVATE rsf_build_options)
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
    if(Vulkan_INCLUDE_DIR)
        rsf_census_layout_check(vk RSF_CENSUS_LAYOUT_VK "${Vulkan_INCLUDE_DIR}/vulkan/vulkan.h" "${Vulkan_INCLUDE_DIR}")
    endif()
    foreach(rsf_layout IN ITEMS sl ffx xess fsr2 vk)
        if(TEST vendor_census_layouts_${rsf_layout})
            set_tests_properties(vendor_census_layouts_${rsf_layout} PROPERTIES SKIP_RETURN_CODE 77)
        endif()
    endforeach()
endif()

add_executable(rsf_sharpening_test graphics_fixture_main.c)
target_compile_definitions(rsf_sharpening_test PRIVATE RSF_GRAPHICS_FIXTURE=rsf_sharpening_fixture)
target_link_libraries(rsf_sharpening_test PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME sharpening COMMAND rsf_sharpening_test)
set_tests_properties(sharpening PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)

add_executable(rsf_sharpening_settings_test sharpening_settings.cpp)
target_link_libraries(rsf_sharpening_settings_test PRIVATE rsf_orchestrator rsf_build_options)
if(EXISTS "${CMAKE_SOURCE_DIR}/vendor/fidelityfx/Kits/FidelityFX/upscalers/include/ffx_upscale.h")
    target_include_directories(rsf_sharpening_settings_test SYSTEM PRIVATE "${CMAKE_SOURCE_DIR}/vendor/fidelityfx/Kits/FidelityFX/upscalers/include")
    target_compile_definitions(rsf_sharpening_settings_test PRIVATE RSF_SHARPENING_FFX_HEADER)
endif()
add_test(NAME sharpening_settings COMMAND rsf_sharpening_settings_test)

add_executable(rsf_sharpening_d3d12_test sharpening_d3d12.cpp)
target_link_libraries(rsf_sharpening_d3d12_test PRIVATE ReScaleFrame::Graphics rsf_build_options)
add_test(NAME sharpening_d3d12 COMMAND rsf_sharpening_d3d12_test)
set_tests_properties(sharpening_d3d12 PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)

if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h")
    add_executable(rsf_sharpening_vulkan_test sharpening_vulkan.cpp)
    target_include_directories(rsf_sharpening_vulkan_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_sharpening_vulkan_test PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME sharpening_vulkan COMMAND rsf_sharpening_vulkan_test)
    set_tests_properties(sharpening_vulkan PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 120)
    add_executable(rsf_sharpening_vulkan_cpu_test sharpening_vulkan_cpu.cpp)
    target_include_directories(rsf_sharpening_vulkan_cpu_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_sharpening_vulkan_cpu_test PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME sharpening_vulkan_cpu COMMAND rsf_sharpening_vulkan_cpu_test)
endif()

if(TARGET rsf_godot AND TARGET rsf_unity_package)
    set(rsf_engine_sharpening_exe "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>/rsf_engine_sharpening_test.exe")
    set(rsf_engine_sharpening_source "${CMAKE_CURRENT_SOURCE_DIR}/engine_sharpening.rs")
    add_custom_command(OUTPUT "${rsf_engine_sharpening_exe}"
        COMMAND "${CMAKE_COMMAND}" -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>"
        COMMAND "${RSF_TEST_RUSTC}" --edition=2024 --target "${rsf_test_rust_target}"
            -D warnings "${rsf_engine_sharpening_source}" -o "${rsf_engine_sharpening_exe}"
        DEPENDS "${rsf_engine_sharpening_source}" engine_sharpening_abi.rs ../sdk/game/src/sharpening_abi.rs
        WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
        VERBATIM)
    add_custom_target(rsf_engine_sharpening_test ALL DEPENDS "${rsf_engine_sharpening_exe}")
    add_dependencies(rsf_engine_sharpening_test rsf_godot rsf_unity_package)
    add_test(NAME engine_sharpening_settings COMMAND ${CMAKE_CROSSCOMPILING_EMULATOR}
        "${rsf_engine_sharpening_exe}" "$<TARGET_FILE:rsf_godot>" "$<TARGET_FILE:rsf_unity_package>")
endif()


add_executable(rsf_capture_metadata capture_metadata.cpp)
target_link_libraries(rsf_capture_metadata PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options windowscodecs ole32)
add_test(NAME capture_metadata_contract COMMAND rsf_capture_metadata "${CMAKE_CURRENT_BINARY_DIR}")

add_executable(rsf_input_receipts_test input_receipts.cpp)
target_compile_definitions(rsf_input_receipts_test PRIVATE RSF_RUNTIME_BUILD)
target_link_libraries(rsf_input_receipts_test PRIVATE ReScaleFrame::GameSDK rsf_native_cpu rsf_build_options)
add_test(NAME input_receipts_contract COMMAND rsf_input_receipts_test)
if(EXISTS "${RSF_XESS_INCLUDE}/xell/xell_d3d12.h" AND
   EXISTS "${RSF_XESS_INCLUDE}/xess_fg/xefg_swapchain_d3d12.h")
    add_executable(rsf_xell_sleep_concurrency xell_sleep_concurrency.cpp)
    target_compile_definitions(rsf_xell_sleep_concurrency PRIVATE RSF_HAVE_XESS_FG=1)
    target_include_directories(rsf_xell_sleep_concurrency SYSTEM PRIVATE "${RSF_XESS_INCLUDE}")
    target_link_libraries(rsf_xell_sleep_concurrency PRIVATE rsf_orchestrator_rust ReScaleFrame::Contract ReScaleFrame::RuntimeCommon rsf_build_options d3d11 d3d12 dxgi)
    add_test(NAME xell_sleep_concurrency_contract COMMAND rsf_xell_sleep_concurrency)
    set_tests_properties(xell_sleep_concurrency_contract PROPERTIES TIMEOUT 10)
endif()
