include("${CMAKE_CURRENT_SOURCE_DIR}/fixture.cmake")
add_library(rsf_sdk_c_header OBJECT sdk_c_header.c)

# Most fixture crates build one archive with every fixture feature: each executable names its entry
# symbol at compile time, so the linker pulls in only what it references. Rust codegen units mix
# modules, though, so crates whose features reach C leaves that only some executables link build an
# archive per feature, all in the one Cargo target directory.
# rsf-presentation-fixtures: the GL fixture calls the Minecraft observer, which only the GL archive carries.
# Each archive carries the Runtime crate (CARRIES, cmake/rust_static.cmake), so its executables drop the
# Runtime, common and graphics archives.
set(rsf_runtime_carried "rsf_orchestrator_rust;rsf_common_rust;rsf_graphics_rust")
rsf_add_rust_static(rsf_presentation_fixtures_rust rsf_rust_presentation_fixtures
    rsf-presentation-fixtures rsf_presentation_fixtures CARRIES "${rsf_runtime_carried}")
rsf_add_rust_static(rsf_presentation_gl_rust rsf_rust_presentation_gl
    rsf-presentation-fixtures rsf_presentation_fixtures --features gl-device-fixture
    CARRIES "${rsf_runtime_carried};rsf_minecraft_rust")
rsf_add_rust_static(rsf_presentation_d3d11_host_device_rust rsf_rust_presentation_d3d11_host_device
    rsf-presentation-fixtures rsf_presentation_fixtures --features d3d11-host-device-fixtures
    CARRIES "${rsf_runtime_carried}")
# With rsf-ac7-plugin (warp-fixture) for the present bridge launcher, which needs both and could not link
# two Rust archives that each carry rsf-ac7's exports.
rsf_add_rust_static(rsf_presentation_ac7_rust rsf_rust_presentation_ac7
    rsf-presentation-fixtures rsf_presentation_fixtures --features ac7-render-scope
    CARRIES "${rsf_runtime_carried};rsf_ac7_plugin_rust")
# The crate links rsf-ac7, whose C leaves are rsf_ac7_leaves.
foreach(archive rsf_presentation_fixtures_rust rsf_presentation_gl_rust rsf_presentation_d3d11_host_device_rust
        rsf_presentation_ac7_rust)
    target_link_libraries(${archive} INTERFACE rsf_ac7_leaves)
endforeach()
# rsf-game-sdk-fixtures reaches the Runtime, Unity and Anomaly crates; one archive with every feature
# gave each executable Unity and Runtime leaves it does not link, and the Unity SR fixture's main.
# rsf_game_sdk_fixtures_rust is the crate without features: the plugin, Anomaly and domain contracts.
# anomaly-sr-device-fixture adds the Anomaly SR temporal device fixture, which calls the Runtime's motion passes.
foreach(variant "fixtures" "render_host;render-host-fixtures" "cpu;cpu-fixtures"
        "vk_latency;vk-latency-fixtures" "unity_sr_d3d12;unity-sr-d3d12-fixture" "d3d11_host;d3d11-host-fixture"
        "anomaly_sr;anomaly-sr-device-fixture")
    list(GET variant 0 name)
    set(features)
    set(carried "${rsf_runtime_carried}")
    if(variant MATCHES ";")
        list(GET variant 1 feature)
        set(features --features ${feature})
    endif()
    if(name STREQUAL "unity_sr_d3d12")
        list(APPEND carried rsf_rdr2_rust rsf_loader_rust)
    endif()
    rsf_add_rust_static(rsf_game_sdk_${name}_rust rsf_rust_game_sdk_${name}
        rsf-game-sdk-fixtures rsf_game_sdk_fixtures ${features} CARRIES "${carried}")
endforeach()
# The archives carry the Rust Runtime, so they take the C leaves and MinHook rsf_orchestrator_rust takes,
# and rsf-anomaly, so the adapter's C leaves its Rust names and the Common Controls 6 manifest its window
# leaves need. The D3D11 state leaves those C leaves call come from the Runtime the archives embed.
set(rsf_anomaly_comctl6 "${PROJECT_SOURCE_DIR}/games/anomaly/comctl6.manifest")
foreach(archive rsf_game_sdk_fixtures_rust rsf_game_sdk_render_host_rust rsf_game_sdk_cpu_rust
        rsf_game_sdk_vk_latency_rust rsf_game_sdk_unity_sr_d3d12_rust rsf_game_sdk_d3d11_host_rust
        rsf_game_sdk_anomaly_sr_rust)
    target_link_libraries(${archive} INTERFACE rsf_runtime_leaves minhook rsf_anomaly_native_leaves)
    if(MSVC)
        target_sources(${archive} INTERFACE "${rsf_anomaly_comctl6}")
    endif()
endforeach()
# rsf-vulkan-fg-fixtures: each provider fixture reaches its own SDK leaves and Runtime entries.
foreach(feature xess-sr xess-input-range xess-device)
    string(MAKE_C_IDENTIFIER "${feature}" name)
    rsf_add_rust_static(rsf_vulkan_fg_${name}_rust rsf_rust_vulkan_fg_${name}
        rsf-vulkan-fg-fixtures rsf_vulkan_fg_fixtures --features ${feature} CARRIES "${rsf_runtime_carried}")
    # The crate links the Rust Runtime, so its archives take what rsf_orchestrator_rust takes.
    target_link_libraries(rsf_vulkan_fg_${name}_rust INTERFACE rsf_runtime_leaves minhook)
endforeach()
# rsf-capture-fixtures: each capture path calls the Runtime's dump entries for its API, which only its
# own executable links. The archives that executables link beside the Runtime archive carry the Runtime;
# vulkan-cpu serves a launcher that imports the Runtime DLL instead (vulkan-cpu-linked is the linked one), and
# texture links only the graphics leaves.
foreach(variant "metadata;runtime" "d3d11;runtime" "d3d12;runtime" "vulkan;runtime" "vulkan-cpu;none"
        "vulkan-cpu-linked;runtime" "texture;graphics")
    list(GET variant 0 feature)
    list(GET variant 1 carries)
    string(MAKE_C_IDENTIFIER "${feature}" name)
    set(carried)
    if(carries STREQUAL "runtime")
        set(carried "${rsf_runtime_carried}")
    elseif(carries STREQUAL "graphics")
        set(carried rsf_graphics_rust)
    endif()
    rsf_add_rust_static(rsf_capture_${name}_rust rsf_rust_capture_${name}
        rsf-capture-fixtures rsf_capture_fixtures --features ${feature} CARRIES "${carried}")
endforeach()
# rsf-ac7-plugin with the engine-address overrides the WARP fixtures need. It replaces the shipping
# rsf_ac7_plugin_rust archive in those executables, after rsf_ac7_render_scope, which does not link either.
rsf_add_rust_static(rsf_ac7_fixture_rust rsf_rust_ac7_fixture rsf-ac7-plugin rsf_ac7_plugin --features warp-fixture
    CARRIES rsf_graphics_rust)
target_link_libraries(rsf_ac7_fixture_rust INTERFACE rsf_ac7_leaves ReScaleFrame::Graphics minhook bcrypt)
target_include_directories(rsf_ac7_fixture_rust INTERFACE "${PROJECT_SOURCE_DIR}/games/ac7/include")
rsf_add_rust_static(rsf_anomaly_fixtures_rust rsf_rust_anomaly_fixtures
    rsf-anomaly rsf_anomaly
    --features fg-fixture,input-hook-fixture,overlay-device-fixture,motion-hooks-fixture,motion-binder-fixture,motion-device-fixture,native-hook-fixture,sr-fixtures
    CARRIES rsf_graphics_rust)
# rsf-minhook-sys leaves MinHook to the CMake link; native_memory.h includes the GameSDK headers. The archive names the
# adapter's C leaves directly, so every executable that links it compiles them in (games/anomaly/CMakeLists.txt).
# The archive carries the D3D11 state leaves those call (rsf-graphics).
target_link_libraries(rsf_anomaly_fixtures_rust INTERFACE minhook ReScaleFrame::GameSDK rsf_anomaly_native_leaves)
if(MSVC)
    target_sources(rsf_anomaly_fixtures_rust INTERFACE "${rsf_anomaly_comctl6}")
endif()
include("${CMAKE_CURRENT_SOURCE_DIR}/d3d11_host_rendered_fg_device.cmake")
# Explicit regression investigation only: requires a scratch vendor-SDK root and NVIDIA GPU.
add_executable(rsf_d3d11_host_shared_sr_device EXCLUDE_FROM_ALL d3d11_host_shared_sr_device.c)
set_target_properties(rsf_d3d11_host_shared_sr_device PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_d3d11_host_shared_sr_device PRIVATE
    rsf_presentation_d3d11_host_device_rust rsf_orchestrator rsf_build_options d3d11 dxgi dxguid)
# Executables that load a built DLL in a process of their own: the Vulkan exports, the Anomaly cold
# start, the engine sharpening settings and the isolated Mono host.
set(rsf_process_dir "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>")
rsf_add_rust_artifacts(rsf_rust_process_fixtures rsf-process-fixtures "${rsf_process_dir}" ALL
    FILES rsf_native_vulkan_exports_test.exe rsf_anomaly_bootstrap_test.exe
        rsf_engine_sharpening_test.exe rsf_unity_mono_runtime_test.exe)
add_dependencies(rsf_rust_process_fixtures rsf_orchestrator rsf_game_anomaly)
add_test(NAME native_vulkan_dll_exports COMMAND ${CMAKE_CROSSCOMPILING_EMULATOR}
    "${rsf_process_dir}/rsf_native_vulkan_exports_test.exe" "$<TARGET_FILE:rsf_orchestrator>")
# Corpus parity run by hand, documented in runtime/shader_patch/README.md; the device test below is the CTest one.
rsf_add_fixture_test(rsf_shader_patch_test ENTRY rsf_shader_patch_corpus_main LIBS rsf_shader_patch MANUAL)
rsf_add_fixture_test(rsf_shader_patch_device_test NAME shader_patch_synthetic_device
    ENTRY rsf_shader_patch_device_main LIBS rsf_shader_patch)
include("${CMAKE_CURRENT_SOURCE_DIR}/d3d11_present_api.cmake")
rsf_add_fixture_test(rsf_native_window_test NAME native_window_contract ENTRY rsf_test_native_window
    LIBS rsf_game_sdk_render_host_rust "$<TARGET_LINKER_FILE:rsf_orchestrator>")
add_dependencies(rsf_native_window_test rsf_orchestrator)
rsf_add_rust_static(rsf_generation_fixtures_rust rsf_rust_generation_fixtures
    rsf-generation-fixtures rsf_generation_fixtures)
include("${CMAKE_CURRENT_SOURCE_DIR}/d3d11_fg_api.cmake")
include("${CMAKE_CURRENT_SOURCE_DIR}/overlay_dll_api.cmake")
include("${CMAKE_CURRENT_SOURCE_DIR}/anomaly_support.cmake")
include("${CMAKE_CURRENT_SOURCE_DIR}/anomaly_fg.cmake")
include("${CMAKE_CURRENT_SOURCE_DIR}/thin_loader.cmake")
rsf_add_fixture_test(rsf_anomaly_plugin_test NAME anomaly_plugin_contract ENTRY rsf_test_anomaly_plugin
    LIBS rsf_game_sdk_fixtures_rust ReScaleFrame::GameSDK ARGS "$<TARGET_FILE:rsf_game_anomaly>")
add_dependencies(rsf_anomaly_plugin_test rsf_game_anomaly)
rsf_add_fixture_test(rsf_anomaly_lifecycle_test NAME anomaly_lifecycle_contract ENTRY rsf_test_anomaly_lifecycle
    LIBS rsf_game_sdk_fixtures_rust ReScaleFrame::GameSDK ARGS "$<TARGET_FILE:rsf_game_anomaly>")
add_dependencies(rsf_anomaly_lifecycle_test rsf_game_anomaly)
add_executable(rsf_anomaly_native_hooks_test anomaly_native_hook_leaves.c)
target_link_libraries(rsf_anomaly_native_hooks_test PRIVATE rsf_anomaly_fixtures_rust ReScaleFrame::GameSDK minhook rsf_build_options)
add_test(NAME anomaly_native_hooks_contract COMMAND rsf_anomaly_native_hooks_test)
set_tests_properties(anomaly_native_hooks_contract PROPERTIES TIMEOUT 30)
add_test(NAME anomaly_bootstrap_contract COMMAND ${CMAKE_CROSSCOMPILING_EMULATOR}
    "${rsf_process_dir}/rsf_anomaly_bootstrap_test.exe"
    "$<TARGET_FILE:rsf_game_anomaly>" "${CMAKE_CURRENT_BINARY_DIR}/anomaly-bootstrap")
set_tests_properties(anomaly_bootstrap_contract PROPERTIES TIMEOUT 130)
find_program(RSF_PWSH_EXECUTABLE pwsh)
if(RSF_PWSH_EXECUTABLE)
    add_test(NAME ffx_line_width_patch COMMAND "${RSF_PWSH_EXECUTABLE}" -NoProfile -File "${CMAKE_CURRENT_SOURCE_DIR}/ffx_line_width_patch.ps1" -SdkRoot "${RSF_FFX_VK_INCLUDE}/../..")
    set_tests_properties(ffx_line_width_patch PROPERTIES SKIP_RETURN_CODE 77 TIMEOUT 30)
endif()
target_link_libraries(rsf_sdk_c_header PRIVATE ReScaleFrame::GameSDK rsf_build_options)

rsf_add_fixture_test(rsf_plugin_contract ENTRY rsf_test_plugin_contract LIBS rsf_game_sdk_fixtures_rust ReScaleFrame::GameSDK)
# Each plugin is loaded by name from its own directory. That is how it is loaded in practice, and it
# keeps the test runnable under a cross-build emulator, which cannot take a host path.
function(rsf_add_plugin_contract test target id)
    add_dependencies(rsf_plugin_contract ${target})
    add_test(NAME ${test} COMMAND rsf_plugin_contract "$<TARGET_FILE_NAME:${target}>" ${id})
    set_tests_properties(${test} PROPERTIES WORKING_DIRECTORY "$<TARGET_FILE_DIR:${target}>")
endfunction()
rsf_add_plugin_contract(game_plugin_contract rsf_game_ac7 ac7)
rsf_add_plugin_contract(game_fallout4_plugin_contract rsf_game_fallout4 fallout4)
rsf_add_plugin_contract(game_skyrim_se_plugin_contract rsf_game_skyrim_se skyrim-se)
rsf_add_plugin_contract(game_project_wingman_plugin_contract rsf_game_project_wingman project-wingman)
rsf_add_plugin_contract(game_rdr2_plugin_contract rsf_game_rdr2 rdr2)
rsf_add_plugin_contract(game_unity_mono_plugin_contract rsf_game_unity_mono unity-mono)
add_executable(rsf_unity_sr_d3d12_test unity_sr_d3d12_preset.c)
set_target_properties(rsf_unity_sr_d3d12_test PROPERTIES LINKER_LANGUAGE CXX)
# The toolchain CRT calls the C ABI main from the fixture archive.
if(MSVC)
    target_link_options(rsf_unity_sr_d3d12_test PRIVATE "LINKER:/SUBSYSTEM:CONSOLE" "LINKER:/INCLUDE:main")
elseif(MINGW)
    target_link_options(rsf_unity_sr_d3d12_test PRIVATE "LINKER:--undefined=main")
endif()
target_include_directories(rsf_unity_sr_d3d12_test PRIVATE ../integrations/unity/include)
# The archive carries the RDR2 runtime setup the fixture calls (rsf-rdr2).
target_link_libraries(rsf_unity_sr_d3d12_test PRIVATE rsf_game_sdk_unity_sr_d3d12_rust rsf_orchestrator rsf_build_options minhook d3d12 dxgi)
# The rsf-unity code in the archive calls the package's Unity SDK leaves.
if(TARGET rsf_unity_package_leaves)
    target_link_libraries(rsf_unity_sr_d3d12_test PRIVATE rsf_unity_package_leaves rsf_unity_package_bridge)
endif()
add_test(NAME unity_sr_d3d12_contract COMMAND rsf_unity_sr_d3d12_test)
rsf_add_fixture_test(rsf_render_frontend_test NAME render_frontend_contract ENTRY rsf_test_render_frontend
    LIBS rsf_game_sdk_render_host_rust rsf_orchestrator)
# The one embedded smoke test (OrchestratorCore linked directly, as AC7 links it); the DLL-linked tests prove the exports.
rsf_add_fixture_test(rsf_render_frontend_embedded_test NAME render_frontend_embedded ENTRY rsf_test_render_frontend
    LIBS rsf_game_sdk_render_host_rust ReScaleFrame::OrchestratorCore)
rsf_add_fixture_test(rsf_callback_host_abi_test NAME callback_host_abi ENTRY rsf_test_callback_host_abi
    LIBS rsf_game_sdk_render_host_rust rsf_orchestrator TIMEOUT 30)

rsf_add_fixture_test(rsf_orchestrator_domains NAME orchestrator_domain_contract ENTRY rsf_test_orchestrator_domains
    LIBS rsf_game_sdk_fixtures_rust)
add_dependencies(rsf_orchestrator_domains rsf_orchestrator rsf_game_unity_mono)
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
rsf_add_fixture_test(rsf_gpu_policy_test NAME gpu_sr_policy ENTRY rsf_test_gpu_policy
    LIBS rsf_generation_fixtures_rust rsf_orchestrator)

rsf_add_rust_static(rsf_plugin_session_fixture_rust rsf_rust_plugin_session_fixture
    rsf-game-sdk-fixtures rsf_game_sdk_fixtures --features plugin-session-fixture CARRIES "${rsf_runtime_carried}")
target_link_libraries(rsf_plugin_session_fixture_rust INTERFACE rsf_runtime_leaves minhook)
if(MSVC)
    target_sources(rsf_plugin_session_fixture_rust INTERFACE "${rsf_anomaly_comctl6}")
endif()
add_library(rsf_plugin_session_fixture SHARED plugin_session_fixture_throw.c)
target_link_libraries(rsf_plugin_session_fixture PRIVATE rsf_plugin_session_fixture_rust ReScaleFrame::GameSDK rsf_build_options)
add_executable(rsf_plugin_session_test game-sdk/session_main.c)
# The fixture archive carries the Rust Runtime whose session entries the C main hands to the fixture; the
# Runtime DLL's import library would define them a second time.
target_link_libraries(rsf_plugin_session_test PRIVATE rsf_game_sdk_fixtures_rust rsf_build_options)
add_dependencies(rsf_plugin_session_test rsf_plugin_session_fixture)
add_test(NAME plugin_session_lifetime COMMAND rsf_plugin_session_test $<TARGET_FILE:rsf_plugin_session_fixture>)
set_tests_properties(plugin_session_lifetime PROPERTIES TIMEOUT 30)

if(MSVC)
    rsf_add_loader_test(rsf_plugin_carrier_test rsf_test_xinput_carrier)
    add_dependencies(rsf_plugin_carrier_test rsf_proxy_plugin)
    add_test(NAME plugin_carrier_contract COMMAND rsf_plugin_carrier_test "$<TARGET_FILE:rsf_proxy_plugin>")
    set_tests_properties(plugin_carrier_contract PROPERTIES SKIP_RETURN_CODE 77)
    rsf_add_loader_test(rsf_shared_shim_exports rsf_test_shared_shim_exports)
    add_dependencies(rsf_shared_shim_exports rsf_proxy_dinput8)
    add_test(NAME shared_shim_exports COMMAND rsf_shared_shim_exports "$<TARGET_FILE:rsf_proxy_dinput8>")
endif()
add_executable(rsf_module_dump_entropy module_dump_entropy.c)
target_link_libraries(rsf_module_dump_entropy PRIVATE ReScaleFrame::LoaderDiagnostics rsf_build_options)
add_test(NAME module_dump_entropy COMMAND rsf_module_dump_entropy)

add_executable(rsf_module_dump_self module_dump_self.c)
target_link_libraries(rsf_module_dump_self PRIVATE ReScaleFrame::LoaderDiagnostics rsf_build_options)
target_compile_definitions(rsf_module_dump_self PRIVATE _CRT_SECURE_NO_WARNINGS)
add_test(NAME module_dump_self COMMAND rsf_module_dump_self "${CMAKE_CURRENT_BINARY_DIR}")

# A capture fixture takes the output directory as its only argument. fixture_main_args.c passes the
# command line to the Rust entry; capture_vulkan_fixture_main.c also hands over the linked Vulkan API.
function(rsf_add_capture_fixture target entry source feature)
    string(MAKE_C_IDENTIFIER "${feature}" name)
    add_executable(${target} ${source})
    target_compile_definitions(${target} PRIVATE RSF_FIXTURE_ENTRY=${entry})
    target_link_libraries(${target} PRIVATE rsf_capture_${name}_rust rsf_build_options)
endfunction()

rsf_add_capture_fixture(rsf_texture_dump rsf_fixture_texture_dump fixture_main_args.c texture)
target_link_libraries(rsf_texture_dump PRIVATE ReScaleFrame::GraphicsLeaves ReScaleFrame::Graphics rsf_build_options)
add_test(NAME texture_dump COMMAND rsf_texture_dump "${CMAKE_CURRENT_BINARY_DIR}")

rsf_add_capture_fixture(rsf_input_dump_test rsf_fixture_input_dump_d3d11 fixture_main_args.c d3d11)
add_library(rsf_input_dump_c_header OBJECT input_dump_c_header.c)
target_link_libraries(rsf_input_dump_c_header PRIVATE ReScaleFrame::FrameDump rsf_build_options)
target_link_libraries(rsf_input_dump_test PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options windowscodecs ole32)
add_test(NAME input_dump COMMAND rsf_input_dump_test "${CMAKE_CURRENT_BINARY_DIR}")
set_tests_properties(input_dump PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

rsf_add_capture_fixture(rsf_input_dump_d3d12 rsf_fixture_input_dump_d3d12 fixture_main_args.c d3d12)
target_compile_definitions(rsf_input_dump_d3d12 PRIVATE RSF_FRAME_DUMP_STATIC)
target_link_libraries(rsf_input_dump_d3d12 PRIVATE ReScaleFrame::FrameDump rsf_build_options d3d12 dxgi)
add_test(NAME input_dump_d3d12 COMMAND rsf_input_dump_d3d12 "${CMAKE_CURRENT_BINARY_DIR}")
set_tests_properties(input_dump_d3d12 PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

if(RSF_FRAME_DUMP_VULKAN_INCLUDE)
    rsf_add_capture_fixture(rsf_input_dump_vulkan_cpu rsf_fixture_input_dump_vulkan_cpu
        capture_vulkan_fixture_main.c vulkan-cpu-linked)
    target_link_libraries(rsf_input_dump_vulkan_cpu PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME input_dump_vulkan_cpu_linked COMMAND rsf_input_dump_vulkan_cpu "${CMAKE_CURRENT_BINARY_DIR}")

    add_executable(rsf_input_dump_vulkan_cpu_dll capture_vulkan_fixture_main.c)
    target_compile_definitions(rsf_input_dump_vulkan_cpu_dll PRIVATE RSF_FIXTURE_ENTRY=rsf_fixture_input_dump_vulkan_cpu)
    target_link_libraries(rsf_input_dump_vulkan_cpu_dll PRIVATE rsf_capture_vulkan_cpu_rust)
    target_include_directories(rsf_input_dump_vulkan_cpu_dll PRIVATE
        "$<TARGET_PROPERTY:rsf_frame_dump,INTERFACE_INCLUDE_DIRECTORIES>")
    target_include_directories(rsf_input_dump_vulkan_cpu_dll SYSTEM PRIVATE "${RSF_FRAME_DUMP_VULKAN_INCLUDE}")
    target_link_libraries(rsf_input_dump_vulkan_cpu_dll PRIVATE "$<TARGET_LINKER_FILE:rsf_orchestrator>"
        ReScaleFrame::GameSDK ReScaleFrame::Graphics ReScaleFrame::Contract rsf_build_options)
    add_dependencies(rsf_input_dump_vulkan_cpu_dll rsf_orchestrator)
    add_test(NAME input_dump_vulkan_cpu_dll COMMAND rsf_input_dump_vulkan_cpu_dll "${CMAKE_CURRENT_BINARY_DIR}")

    rsf_add_capture_fixture(rsf_input_dump_vulkan rsf_fixture_input_dump_vulkan
        capture_vulkan_fixture_main.c vulkan)
    target_include_directories(rsf_input_dump_vulkan SYSTEM PRIVATE "${RSF_FRAME_DUMP_VULKAN_INCLUDE}")
    target_compile_definitions(rsf_input_dump_vulkan PRIVATE RSF_RUNTIME_BUILD)
    target_link_libraries(rsf_input_dump_vulkan PRIVATE ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME input_dump_vulkan COMMAND rsf_input_dump_vulkan "${CMAKE_CURRENT_BINARY_DIR}")
    set_tests_properties(input_dump_vulkan PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)
endif()

rsf_add_fixture_test(rsf_frame_assembly NAME frame_assembly ENTRY rsf_test_frame_assembly
    LIBS rsf_generation_fixtures_rust rsf_orchestrator)

add_executable(rsf_ac7_velocity_shader_test ac7_velocity_shader.c)
set_target_properties(rsf_ac7_velocity_shader_test PROPERTIES LINKER_LANGUAGE CXX)
if(MINGW)
    target_link_options(rsf_ac7_velocity_shader_test PRIVATE -municode)
endif()
target_link_libraries(rsf_ac7_velocity_shader_test PRIVATE ReScaleFrame::AC7RenderScope rsf_ac7_fixture_rust minhook ReScaleFrame::Graphics ReScaleFrame::RuntimeCommon
    rsf_build_options d3d11)
add_test(NAME ac7_velocity_shader COMMAND rsf_ac7_velocity_shader_test)
set_tests_properties(ac7_velocity_shader PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

rsf_add_rust_static(rsf_ac7_capture_fixture_rust rsf_rust_ac7_capture_fixture
    rsf-ac7-capture-fixture rsf_ac7_capture_fixture CARRIES "${rsf_runtime_carried}")
# The fixture installs the Runtime's D3D11 observer; its archive carries rsf-ac7 and the Runtime, which
# also supplies the D3D11 leaves, so the executable takes the Runtime's C leaves without its archive.
target_link_libraries(rsf_ac7_capture_fixture_rust INTERFACE rsf_ac7_leaves)
target_include_directories(rsf_ac7_capture_fixture_rust INTERFACE "${PROJECT_SOURCE_DIR}/games/ac7/include")
rsf_add_fixture_test(rsf_ac7_capture_io NAME ac7_capture_io ENTRY rsf_test_ac7_capture_io
    LIBS rsf_ac7_capture_fixture_rust rsf_orchestrator_rust ReScaleFrame::Graphics ReScaleFrame::GameSDK d3dcompiler
    TIMEOUT 120 SKIP77)

# rsf-ac7 with the view probe and session launchers' Rust halves; it replaces the shipping
# ReScaleFrame::AC7View archive in those executables.
rsf_add_rust_static(rsf_ac7_view_fixture_rust rsf_rust_ac7_view_fixture rsf-ac7 rsf_ac7 --features view-fixtures
    CARRIES rsf_graphics_rust)
# As rsf_ac7_rust: the crate's C leaves and public headers. The probe reads its captures through the
# C leaf ac7_view_capture.c, which the session launcher's archive carries the probe code for as well.
target_link_libraries(rsf_ac7_view_fixture_rust INTERFACE rsf_ac7_leaves ReScaleFrame::Graphics)
target_sources(rsf_ac7_view_fixture_rust INTERFACE "${CMAKE_CURRENT_SOURCE_DIR}/ac7_view_capture.c")
target_include_directories(rsf_ac7_view_fixture_rust INTERFACE "${PROJECT_SOURCE_DIR}/games/ac7/include")
add_executable(rsf_ac7_view_test ac7_view.c)
set_target_properties(rsf_ac7_view_test PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_ac7_view_test PRIVATE rsf_ac7_view_fixture_rust rsf_build_options)
add_test(NAME ac7_view COMMAND rsf_ac7_view_test)

rsf_add_fixture_test(rsf_ac7_session NAME ac7_session ENTRY rsf_ac7_session_fixture CXX
    LIBS rsf_ac7_view_fixture_rust)

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

# The composite's arithmetic.
rsf_add_fixture_test(rsf_fullscreen_pass NAME fullscreen_pass ENTRY rsf_fullscreen_pass_fixture
    LIBS rsf_orchestrator_rust ReScaleFrame::Graphics TIMEOUT 120 SKIP77)

rsf_add_fixture_test(rsf_native_reinsert NAME native_reinsert_device ENTRY rsf_native_reinsert_fixture
    LIBS rsf_orchestrator_rust ReScaleFrame::Graphics ReScaleFrame::GameSDK DEFINES RSF_RUNTIME_BUILD
    TIMEOUT 120 SKIP77)
target_include_directories(rsf_native_reinsert PRIVATE ../runtime/orchestrator/include)

rsf_add_fixture_test(rsf_depth_scale NAME depth_scale_device ENTRY rsf_depth_scale_fixture
    LIBS rsf_orchestrator_rust ReScaleFrame::Graphics TIMEOUT 120 SKIP77)

rsf_add_fixture_test(rsf_auxiliary_sr NAME auxiliary_sr_device ENTRY rsf_auxiliary_sr_fixture
    LIBS rsf_orchestrator_rust ReScaleFrame::Graphics ReScaleFrame::GameSDK TIMEOUT 60 SKIP77)

rsf_add_rust_static(rsf_skyrim_se_fixture_rust rsf_rust_skyrim_se_fixture rsf-skyrim-se rsf_skyrim_se --features fixtures)
target_link_libraries(rsf_skyrim_se_fixture_rust INTERFACE rsf_skyrim_se_native)
add_executable(rsf_skyrim_input skyrim_input_main.c ../games/skyrim-se/src/guarded_read.c)
set_target_properties(rsf_skyrim_input PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON LINKER_LANGUAGE CXX)
target_link_libraries(rsf_skyrim_input PRIVATE rsf_skyrim_se_fixture_rust ReScaleFrame::GameSDK rsf_steam_native minhook rsf_build_options)
add_test(NAME skyrim_input_contract COMMAND rsf_skyrim_input)

rsf_add_fixture_test(rsf_native_cpu_reservation_test NAME native_cpu_reservation_contract
    ENTRY rsf_cpu_reservation_fixture LIBS rsf_game_sdk_cpu_rust rsf_native_cpu)

rsf_add_fixture_test(rsf_native_fg_cpu_test NAME native_fg_cpu_contract ENTRY rsf_native_fg_cpu_fixture
    LIBS ReScaleFrame::OrchestratorCore TIMEOUT 15)

rsf_add_fixture_test(rsf_sampler_bias NAME sampler_bias_device ENTRY rsf_sampler_bias_fixture
    LIBS rsf_orchestrator_rust ReScaleFrame::Graphics ReScaleFrame::GameSDK TIMEOUT 60 SKIP77)
target_include_directories(rsf_sampler_bias PRIVATE ../runtime/orchestrator/include ../runtime/presentation/include)

add_executable(rsf_d3d11_host_test d3d11_host_c_header.c)
set_target_properties(rsf_d3d11_host_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON
    LINKER_LANGUAGE CXX)
# The fixture archive carries the Rust Runtime, so the host calls and the two frame-record helpers the C
# main checks resolve in it; the Runtime DLL's import library would define those helpers a second time.
target_include_directories(rsf_d3d11_host_test PRIVATE "$<TARGET_PROPERTY:rsf_orchestrator,INTERFACE_INCLUDE_DIRECTORIES>")
target_link_libraries(rsf_d3d11_host_test PRIVATE rsf_game_sdk_d3d11_host_rust rsf_build_options)
add_test(NAME d3d11_host_contract COMMAND rsf_d3d11_host_test)


rsf_add_fixture_test(rsf_motion_decode NAME motion_decode ENTRY rsf_motion_decode_fixture
    LIBS rsf_orchestrator_rust ReScaleFrame::Graphics TIMEOUT 120)

rsf_add_fixture_test(rsf_d3d11_state NAME d3d11_state ENTRY rsf_d3d11_state_fixture
    LIBS rsf_orchestrator_rust ReScaleFrame::Graphics TIMEOUT 120)


add_executable(rsf_d3d11_observer d3d11_observer.c)
set_target_properties(rsf_d3d11_observer PROPERTIES LINKER_LANGUAGE CXX)
# The observer is implemented in the Runtime archive.
target_link_libraries(rsf_d3d11_observer PRIVATE rsf_orchestrator_rust ReScaleFrame::Graphics rsf_build_options)
add_test(NAME d3d11_observer COMMAND rsf_d3d11_observer "${CMAKE_CURRENT_BINARY_DIR}")
# These touch a graphics runtime and patch vtables, so a deadlock is a plausible failure. Fail in
# a couple of minutes rather than hanging the suite.
set_tests_properties(d3d11_observer texture_dump PROPERTIES TIMEOUT 120)

set(overlay_fixture_dll "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>/rsf_overlay_panel_fixture.dll")
rsf_add_rust_artifacts(rsf_overlay_panel_fixture rsf-overlay-panel-fixture "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>"
    FILES rsf_overlay_panel_fixture.dll)
rsf_add_rust_static(rsf_overlay_fixtures_rust rsf_rust_overlay_fixtures
    rsf-overlay-fixtures rsf_overlay_fixtures CARRIES "${rsf_runtime_carried}")
rsf_add_fixture_test(rsf_overlay_host ENTRY rsf_test_overlay_host
    LIBS rsf_overlay_fixtures_rust rsf_orchestrator_rust ReScaleFrame::Graphics)

rsf_add_fixture_test(rsf_overlay_hotkey NAME overlay_hotkey ENTRY rsf_test_overlay_hotkey
    LIBS rsf_overlay_fixtures_rust rsf_orchestrator_rust ReScaleFrame::Graphics TIMEOUT 30)
add_dependencies(rsf_overlay_host rsf_overlay_panel_fixture)
add_test(NAME overlay_host COMMAND rsf_overlay_host "${overlay_fixture_dll}")
set_tests_properties(overlay_host PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77
    WORKING_DIRECTORY "$<TARGET_FILE_DIR:rsf_overlay_host>")
rsf_add_fixture_test(rsf_overlay_native12_test NAME overlay_native12_backpressure
    ENTRY rsf_test_overlay_native12_run SOURCES overlay_native12.c ARGS "${overlay_fixture_dll}"
    LIBS rsf_overlay_fixtures_rust rsf_orchestrator_rust ReScaleFrame::Graphics d3d12 d3dcompiler dxgi
    TIMEOUT 30 SKIP77)
target_include_directories(rsf_overlay_native12_test PRIVATE ../runtime/orchestrator/include)
add_dependencies(rsf_overlay_native12_test rsf_overlay_panel_fixture)

# Loading a vendor runtime that talks to a driver is another place a hang is plausible.
rsf_add_fixture_test(rsf_dlss_backend NAME dlss_backend ENTRY rsf_fixture_dlss_backend
    LIBS ReScaleFrame::DLSS TIMEOUT 120)

# Rust builds the Streamline records for DLSS SR. sl.h is C++ only, so a C++ translation unit
# reports the SDK's layouts, defaults and enum values and the Rust fixture compares them.
if(RSF_HAVE_STREAMLINE)
    add_executable(rsf_dlss_sr_layout dlss_sr_layout_main.c dlss_sr_layout_facts.cpp)
    target_include_directories(rsf_dlss_sr_layout SYSTEM PRIVATE "${RSF_STREAMLINE_INCLUDE}")
    target_link_libraries(rsf_dlss_sr_layout PRIVATE ReScaleFrame::DLSS rsf_build_options d3d12)
    add_test(NAME dlss_sr_layout COMMAND rsf_dlss_sr_layout)
endif()

rsf_add_fixture_test(rsf_sr_bridge_test NAME sr_bridge ENTRY rsf_sr_bridge_fixture ARGV
    LIBS ReScaleFrame::OrchestratorCore SKIP77 TIMEOUT 120)

# The Rust fixture injects deterministic providers without opening vendor DLLs.
rsf_add_fixture_test(rsf_sr_session_test NAME sr_session ENTRY rsf_sr_session_fixture
    LIBS ReScaleFrame::OrchestratorCore)

rsf_add_fixture_test(rsf_motion_resolve_test NAME motion_resolve ENTRY rsf_motion_resolve_fixture
    LIBS rsf_orchestrator_rust ReScaleFrame::Graphics SKIP77 TIMEOUT 120)

rsf_add_fixture_test(rsf_colour_transport_test NAME colour_transport ENTRY rsf_colour_transport_fixture
    LIBS rsf_orchestrator_rust ReScaleFrame::Graphics SKIP77 TIMEOUT 120)
rsf_add_rust_static(rsf_translucency_fixtures_rust rsf_rust_translucency_fixtures
    rsf-translucency-fixtures rsf_translucency_fixtures)
# Only import the Runtime. Its PUBLIC dependencies also include the embedded core.
rsf_add_fixture_test(rsf_native_translucency_test NAME native_translucency ENTRY rsf_native_translucency_fixture
    LIBS rsf_translucency_fixtures_rust "$<TARGET_LINKER_FILE:rsf_orchestrator>" d3d11 SKIP77 TIMEOUT 120)
rsf_add_fixture_test(rsf_native_translucency_abi_test NAME native_translucency_abi
    ENTRY rsf_native_translucency_abi_fixture
    LIBS rsf_translucency_fixtures_rust "$<TARGET_LINKER_FILE:rsf_orchestrator>" d3d11)
add_dependencies(rsf_native_translucency_test rsf_orchestrator)
add_dependencies(rsf_native_translucency_abi_test rsf_orchestrator)

rsf_add_fixture_test(rsf_sr_pipeline_test NAME sr_pipeline ENTRY rsf_fixture_sr_pipeline ARGV
    LIBS ReScaleFrame::OrchestratorCore SKIP77 TIMEOUT 120)
add_dependencies(rsf_sr_pipeline_test rsf_orchestrator)

rsf_add_fixture_test(rsf_fg_session_test NAME fg_session ENTRY rsf_test_fg_session
    LIBS rsf_generation_fixtures_rust rsf_orchestrator TIMEOUT 30)

add_executable(rsf_fg12_abi_test fg12_abi.c)
target_link_libraries(rsf_fg12_abi_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME fg12_abi COMMAND rsf_fg12_abi_test)

add_executable(rsf_sr12_abi_test sr12_abi.c)
target_link_libraries(rsf_sr12_abi_test PRIVATE rsf_orchestrator rsf_build_options)
add_test(NAME sr12_abi COMMAND rsf_sr12_abi_test)


rsf_add_fixture_test(rsf_fg_provider_test NAME fg_provider ENTRY rsf_test_fg_provider ARGV
    LIBS rsf_generation_fixtures_rust rsf_orchestrator TIMEOUT 120 SKIP77)

rsf_add_fixture_test(rsf_fg_leases_test NAME fg_leases ENTRY rsf_test_fg_leases
    LIBS rsf_generation_fixtures_rust rsf_orchestrator TIMEOUT 120 SKIP77)

rsf_add_fixture_test(rsf_fg_leases_faults_test NAME fg_leases_faults ENTRY rsf_test_fg_leases_faults
    LIBS rsf_generation_fixtures_rust rsf_orchestrator)

add_library(rsf_fg_c_header OBJECT fg_c_header.c)
target_include_directories(rsf_fg_c_header PRIVATE ../runtime/orchestrator/include)
target_link_libraries(rsf_fg_c_header PRIVATE ReScaleFrame::DLSS rsf_build_options)

rsf_add_fixture_test(rsf_fg_present_bridge_test NAME shared_fg_present_bridge ENTRY rsf_fg_fixture_main ARGV CXX
    LIBS rsf_presentation_fixtures_rust rsf_orchestrator_core TIMEOUT 120 SKIP77)
# The render scope calls other than create are exported by rsf-ac7-plugin (render_scope.rs), which the
# fixture archive carries with ac7-render-scope; the create call is ReScaleFrame::AC7RenderScope's.
rsf_add_fixture_test(rsf_d3d11_present_bridge_test NAME d3d11_present_bridge ENTRY rsf_d3d11_fixture_main ARGV CXX
    LIBS rsf_presentation_ac7_rust rsf_orchestrator_core ReScaleFrame::AC7RenderScope minhook d3d11 dxgi
    TIMEOUT 120 SKIP77)
# Graphics fixtures must not load game-facing proxy DLLs from the shared artifact directory.
set_target_properties(rsf_fg_present_bridge_test rsf_d3d11_present_bridge_test PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/fixtures/fg/$<CONFIG>")
add_dependencies(rsf_d3d11_present_bridge_test rsf_proxy_ac7)
add_test(NAME ac7_proxy_startup COMMAND rsf_d3d11_present_bridge_test
    --proxy-smoke $<TARGET_FILE:rsf_proxy_ac7>)
set_tests_properties(ac7_proxy_startup PROPERTIES TIMEOUT 120)
rsf_add_fixture_test(rsf_fg_switching_test NAME fg_switching ENTRY rsf_test_fg_switching ARGV
    LIBS rsf_generation_fixtures_rust rsf_orchestrator TIMEOUT 120 SKIP77)

rsf_add_fixture_test(rsf_latency_host_test NAME latency_host ENTRY rsf_test_latency_host ARGV
    LIBS rsf_generation_fixtures_rust rsf_orchestrator TIMEOUT 120 SKIP77)

# The latency fixture drives the Rust latency owner with its own FG endpoints and fake Streamline table.
add_executable(rsf_native_latency_vulkan_test fixture_main.c)
target_compile_definitions(rsf_native_latency_vulkan_test PRIVATE RSF_FIXTURE_ENTRY=rsf_native_latency_vulkan_fixture)
target_link_libraries(rsf_native_latency_vulkan_test PRIVATE rsf_game_sdk_vk_latency_rust ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_native_cpu rsf_build_options)
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
# The Godot module is built by the engine with a bare rustc object, so its graph fixture compiles the
# same file the same way.
find_program(RSF_RUSTC rustc REQUIRED)
set(godot_module "${PROJECT_SOURCE_DIR}/integrations/godot/modules/rescaleframe")
set(godot_graph_object "${CMAKE_CURRENT_BINARY_DIR}/godot_graph_module${CMAKE_CXX_OUTPUT_EXTENSION}")
add_custom_command(OUTPUT "${godot_graph_object}"
    COMMAND "${RSF_RUSTC}" --edition=2024 --crate-type=lib --emit=obj --target ${RSF_RUST_TARGET}
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
    add_executable(rsf_native_fg_xess_vulkan_test fixture_main.c)
    set_target_properties(rsf_native_fg_xess_vulkan_test PROPERTIES LINKER_LANGUAGE CXX)
    target_compile_definitions(rsf_native_fg_xess_vulkan_test PRIVATE RSF_FIXTURE_ENTRY=rsf_fixture_xess_interop)
    target_link_libraries(rsf_native_fg_xess_vulkan_test PRIVATE rsf_vulkan_fg_xess_device_rust ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME native_fg_xess_vulkan_contract COMMAND rsf_native_fg_xess_vulkan_test)
endif()
if(RSF_HAVE_VULKAN)
    add_executable(rsf_ffx_vulkan_wsi_dispatch_test ffx_vulkan_wsi_dispatch.c)
    set_target_properties(rsf_ffx_vulkan_wsi_dispatch_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON)
    target_include_directories(rsf_ffx_vulkan_wsi_dispatch_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_ffx_vulkan_wsi_dispatch_test PRIVATE rsf_build_options)
    add_test(NAME ffx_vulkan_wsi_dispatch COMMAND rsf_ffx_vulkan_wsi_dispatch_test)
    if(RSF_HAVE_FFX_VK)
        # vulkan_ffx_fg/fixture.rs in the test-fixtures orchestrator archive: the scenarios replace the SDK module
        add_executable(rsf_native_ffx_vulkan_fixture fixture_main_args.c)
        set_target_properties(rsf_native_ffx_vulkan_fixture PROPERTIES LINKER_LANGUAGE CXX)
        target_compile_definitions(rsf_native_ffx_vulkan_fixture PRIVATE RSF_FIXTURE_ENTRY=rsf_fixture_ffx_run)
        # through their own host table; the provider entries and SDK leaf are the production ones.
        target_link_libraries(rsf_native_ffx_vulkan_fixture PRIVATE rsf_orchestrator_rust
            ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::FrameDump rsf_build_options)
        add_test(NAME native_ffx_vulkan_reset_contract COMMAND rsf_native_ffx_vulkan_fixture 0)
        add_test(NAME native_ffx_vulkan_submission_contract COMMAND rsf_native_ffx_vulkan_fixture 1)
        add_test(NAME native_ffx_vulkan_wsi_route_contract COMMAND rsf_native_ffx_vulkan_fixture 2)
    endif()
endif()
rsf_add_fixture_test(rsf_render_links_test NAME render_links ENTRY rsf_test_render_links
    LIBS rsf_game_sdk_render_host_rust rsf_orchestrator TIMEOUT 120)

rsf_add_rust_static(rsf_ac7_render_scope_fixture_rust rsf_rust_ac7_render_scope_fixture
    rsf-ac7-render-scope-fixture rsf_ac7_render_scope_fixture)
add_executable(rsf_ac7_render_scope_test ac7_render_scope.c)
target_link_libraries(rsf_ac7_render_scope_test PRIVATE rsf_ac7_render_scope_fixture_rust ReScaleFrame::AC7RenderScope rsf_ac7_plugin_rust minhook rsf_build_options)
add_test(NAME ac7_render_scope COMMAND rsf_ac7_render_scope_test)
set_tests_properties(ac7_render_scope PROPERTIES TIMEOUT 120)

# Explicit hardware experiment, never run by routine CTest verification.
rsf_add_rust_static(rsf_exposure_probe_rust rsf_exposure_probe_rust_build rsf-exposure-probe rsf_exposure_probe
    CARRIES "${rsf_runtime_carried}")
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

# The C launcher hands the replay helpers (tests/ac7_view_fixture.rs, mounted in rsf-ac7 by its warp-fixture
# feature), rsf_native_sr_evaluate, the render scope create
# call and the native renderer's call table to the Rust fixture (ac7_native_graph.rs), which the
# rsf-ac7-plugin crate builds with its warp-fixture feature beside games/ac7/plugin/src/native_renderer.rs.
add_executable(rsf_ac7_native_graph_test ac7_native_graph.c)
set_target_properties(rsf_ac7_native_graph_test PROPERTIES LINKER_LANGUAGE CXX)
target_link_libraries(rsf_ac7_native_graph_test PRIVATE ReScaleFrame::AC7RenderScope rsf_ac7_fixture_rust rsf_orchestrator minhook rsf_build_options)
add_test(NAME ac7_native_graph COMMAND rsf_ac7_native_graph_test)
set_tests_properties(ac7_native_graph PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

add_executable(rsf_gl_shared_surface_test presentation/gl_shared_surface_main.c)
set_target_properties(rsf_gl_shared_surface_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON
    LINKER_LANGUAGE CXX)
# The GL archive carries the Minecraft observer and the Runtime.
target_link_libraries(rsf_gl_shared_surface_test PRIVATE rsf_presentation_gl_rust ReScaleFrame::OrchestratorCore rsf_build_options opengl32)
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
target_link_libraries(rsf_native_xess_vulkan_test PRIVATE rsf_vulkan_fg_xess_sr_rust ReScaleFrame::NativeVulkan rsf_build_options)
if(RSF_HAVE_VULKAN AND RSF_HAVE_XESS_VK)
    target_compile_definitions(rsf_native_xess_vulkan_test PRIVATE RSF_TEST_VULKAN=1)
endif()
add_test(NAME native_xess_vulkan_contract COMMAND rsf_native_xess_vulkan_test)
if(RSF_HAVE_VULKAN AND RSF_HAVE_XESS_VK AND RSF_HAVE_FFX_VK)
    add_executable(rsf_native_xess_input_range_test fixture_main.c)
    set_target_properties(rsf_native_xess_input_range_test PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON LINKER_LANGUAGE CXX)
    target_compile_definitions(rsf_native_xess_input_range_test PRIVATE RSF_FIXTURE_ENTRY=rsf_fixture_xess_input_range)
    target_link_libraries(rsf_native_xess_input_range_test PRIVATE rsf_vulkan_fg_xess_input_range_rust ReScaleFrame::NativeVulkan rsf_build_options)
    add_test(NAME native_xess_input_range_contract COMMAND rsf_native_xess_input_range_test)
endif()
# The provider-dispatch contract is the vulkan_fg::dispatch_tests unit test.
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
    # The fixture is vulkan_dx12_fg/fsr4_state_fixture.rs in the fixture-leaves orchestrator archive.
    target_link_libraries(rsf_native_fsr4_fg_state_test PRIVATE
        ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_orchestrator_rust rsf_build_options)
    add_test(NAME native_fsr4_fg_state_contract COMMAND rsf_native_fsr4_fg_state_test)
endif()
add_test(NAME native_xess_vulkan_device COMMAND rsf_native_xess_vulkan_test --device)
set_tests_properties(native_xess_vulkan_device PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)

if(RSF_HAVE_VULKAN)
    add_executable(rsf_streamline_vulkan_dispatch_test streamline_vulkan_dispatch.cpp)
    target_include_directories(rsf_streamline_vulkan_dispatch_test SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_streamline_vulkan_dispatch_test PRIVATE rsf_build_options)
    add_test(NAME streamline_vulkan_dispatch_contract COMMAND rsf_streamline_vulkan_dispatch_test)
    add_executable(rsf_native_antilag_vulkan_test fixture_main.c)
    target_compile_definitions(rsf_native_antilag_vulkan_test PRIVATE RSF_FIXTURE_ENTRY=rsf_native_antilag_vulkan_fixture)
    target_link_libraries(rsf_native_antilag_vulkan_test PRIVATE rsf_game_sdk_vk_latency_rust ReScaleFrame::GameSDK ReScaleFrame::Contract rsf_native_cpu rsf_build_options)
    add_test(NAME native_antilag_vulkan_contract COMMAND rsf_native_antilag_vulkan_test)
    set_tests_properties(native_antilag_vulkan_contract PROPERTIES TIMEOUT 20)
endif()

if(RSF_HAVE_VULKAN AND RSF_HAVE_STREAMLINE)
    add_executable(rsf_streamline_selected_device_test streamline_selected_device.cpp)
    get_filename_component(rsf_streamline_source_root "${RSF_STREAMLINE_INCLUDE}" DIRECTORY)
    target_include_directories(rsf_streamline_selected_device_test SYSTEM PRIVATE "${rsf_streamline_source_root}" "${RSF_STREAMLINE_INCLUDE}")
    target_link_libraries(rsf_streamline_selected_device_test PRIVATE rsf_build_options)
    add_test(NAME streamline_selected_device_contract COMMAND rsf_streamline_selected_device_test)
    # The production DLSS-G table (vulkan_dlss_fg/leaves.rs) and its Rust leaves, with the fixture's
    # Streamline session and SDK entries (vulkan_dlss_fg/fixture.rs in the test-fixtures orchestrator archive).
    add_executable(rsf_native_fg_dlss_vulkan_test fixture_main.c)
    set_target_properties(rsf_native_fg_dlss_vulkan_test PROPERTIES LINKER_LANGUAGE CXX)
    target_compile_definitions(rsf_native_fg_dlss_vulkan_test PRIVATE RSF_FIXTURE_ENTRY=rsf_fixture_dlss_run)
    target_link_libraries(rsf_native_fg_dlss_vulkan_test PRIVATE rsf_orchestrator_rust ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::FrameDump rsf_build_options)
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
    # The fake DLLs' exports have the vendor names the census test imports, so they get an archive of their own.
    rsf_add_rust_static(rsf_loader_vendor_dll_fixtures_rust rsf_rust_loader_vendor_dll_fixtures
        rsf-loader-fixtures rsf_loader_fixtures --features vendor-dll-fixtures CARRIES rsf_loader_rust)
    target_link_libraries(rsf_loader_vendor_dll_fixtures_rust INTERFACE minhook)
    function(rsf_census_fixture target define output)
        string(TOLOWER "${define}" fixture)
        add_library(${target} SHARED loader/vendor_census_fixture.c "loader/${fixture}.def")
        target_compile_definitions(${target} PRIVATE ${define})
        target_link_libraries(${target} PRIVATE rsf_loader_vendor_dll_fixtures_rust rsf_build_options)
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
    function(rsf_census_test name mode timeout)
        add_test(NAME ${name} COMMAND rsf_vendor_census_test ${mode} "${CMAKE_CURRENT_BINARY_DIR}/census-${mode}")
        set_tests_properties(${name} PROPERTIES TIMEOUT ${timeout})
    endfunction()
    rsf_census_test(vendor_census functional 120)
    rsf_census_test(vendor_census_budget budget 120)
    rsf_census_test(vendor_census_vulkan_streamline vulkan 120)
    rsf_census_test(vendor_census_fidelityfx_api ffx 120)
    rsf_census_test(vendor_census_dllmain dllmain 120)
    rsf_census_test(vendor_census_creation_identity creation 30)

    if(TARGET rsf_asi_rdr2)
        rsf_add_rust_static(rsf_rdr2_fixtures_rust rsf_rust_rdr2_fixtures rsf-rdr2 rsf_rdr2
            --features camera-fixture,capture-fixture CARRIES rsf_loader_rust)
        rsf_add_fixture_test(rsf_rdr2_camera NAME rdr2_camera ENTRY rsf_rdr2_camera_fixture
            LIBS rsf_rdr2_fixtures_rust minhook user32
            ARGS "${CMAKE_CURRENT_SOURCE_DIR}/fixtures/rdr2-camera-v1.bin")
        add_executable(rsf_rdr2_capture rdr2_capture_main.c rdr2_capture_ngx.c ../games/rdr2/src/capture_ngx.c)
        target_compile_definitions(rsf_rdr2_capture PRIVATE RSF_RDR2_CAPTURE_FIXTURE)
        target_link_libraries(rsf_rdr2_capture PRIVATE rsf_rdr2_fixtures_rust ReScaleFrame::GameSDK ReScaleFrame::VendorCensus minhook
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
            # Streamline and FSR 2 declare C++ types (constructors, virtual members), so the check is C++.
            add_executable(rsf_vendor_census_layouts_${suffix} loader/vendor_layouts.cpp)
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
    if(RSF_FRAME_DUMP_VULKAN_INCLUDE)
        rsf_census_layout_check(vk RSF_CENSUS_LAYOUT_VK "${RSF_FRAME_DUMP_VULKAN_INCLUDE}/vulkan/vulkan.h" "${RSF_FRAME_DUMP_VULKAN_INCLUDE}")
    endif()
    foreach(rsf_layout IN ITEMS sl ffx xess fsr2 vk)
        if(TEST vendor_census_layouts_${rsf_layout})
            set_tests_properties(vendor_census_layouts_${rsf_layout} PROPERTIES SKIP_RETURN_CODE 77)
        endif()
    endforeach()
endif()

rsf_add_fixture_test(rsf_sharpening_test NAME sharpening ENTRY rsf_sharpening_fixture
    LIBS rsf_orchestrator_rust ReScaleFrame::Graphics SKIP77 TIMEOUT 120)

add_executable(rsf_sharpening_settings_test sharpening_settings_native.c)
set_target_properties(rsf_sharpening_settings_test PROPERTIES LINKER_LANGUAGE CXX)
# The settings fixture and rsf_ffx_sharpening are Runtime internals, not DLL exports.
target_link_libraries(rsf_sharpening_settings_test PRIVATE rsf_orchestrator_rust rsf_build_options)
if(EXISTS "${CMAKE_SOURCE_DIR}/vendor/fidelityfx/Kits/FidelityFX/upscalers/include/ffx_upscale.h")
    target_include_directories(rsf_sharpening_settings_test SYSTEM PRIVATE "${CMAKE_SOURCE_DIR}/vendor/fidelityfx/Kits/FidelityFX/upscalers/include")
    target_compile_definitions(rsf_sharpening_settings_test PRIVATE RSF_SHARPENING_FFX_HEADER)
endif()
add_test(NAME sharpening_settings COMMAND rsf_sharpening_settings_test)

rsf_add_fixture_test(rsf_sharpening_d3d12_test NAME sharpening_d3d12 ENTRY rsf_sharpening_d3d12_fixture
    LIBS rsf_orchestrator_rust ReScaleFrame::Graphics SKIP77 TIMEOUT 120)

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
    add_dependencies(rsf_rust_process_fixtures rsf_godot rsf_unity_package)
    add_test(NAME engine_sharpening_settings COMMAND ${CMAKE_CROSSCOMPILING_EMULATOR}
        "${rsf_process_dir}/rsf_engine_sharpening_test.exe" "$<TARGET_FILE:rsf_godot>" "$<TARGET_FILE:rsf_unity_package>")
endif()

rsf_add_capture_fixture(rsf_capture_metadata rsf_fixture_capture_metadata fixture_main_args.c metadata)
target_link_libraries(rsf_capture_metadata PRIVATE ReScaleFrame::OrchestratorCore rsf_build_options windowscodecs ole32)
add_test(NAME capture_metadata_contract COMMAND rsf_capture_metadata "${CMAKE_CURRENT_BINARY_DIR}")

rsf_add_fixture_test(rsf_input_receipts_test NAME input_receipts_contract ENTRY rsf_input_receipts_fixture
    LIBS rsf_game_sdk_cpu_rust rsf_native_cpu)
if(EXISTS "${RSF_XESS_INCLUDE}/xell/xell_d3d12.h" AND
   EXISTS "${RSF_XESS_INCLUDE}/xess_fg/xefg_swapchain_d3d12.h")
    # d3d12_generation/xell_fixture.rs in the fixture-leaves orchestrator archive.
    rsf_add_fixture_test(rsf_xell_sleep_concurrency NAME xell_sleep_concurrency_contract ENTRY rsf_xell_fixture_run
        LIBS rsf_orchestrator_rust TIMEOUT 10)
endif()

# These executables call the fixture exports of the Rust orchestrator, which only the test-fixtures archive carries
# (runtime/orchestrator/CMakeLists.txt).
foreach(rsf_fixture_target
        rsf_fullscreen_pass rsf_native_reinsert rsf_depth_scale rsf_auxiliary_sr rsf_sampler_bias
        rsf_motion_decode rsf_d3d11_state rsf_motion_resolve_test rsf_colour_transport_test
        rsf_sharpening_test rsf_sharpening_d3d12_test rsf_sharpening_vulkan_test rsf_sharpening_vulkan_cpu_test
        rsf_sharpening_settings_test rsf_exposure_normalization_test rsf_native_vulkan_test rsf_dlss_backend
        rsf_dlss_sr_layout rsf_sr_session_test rsf_sr_bridge_test rsf_sr_pipeline_test rsf_sr_backend_switch_device_test
        rsf_shader_patch_test rsf_shader_patch_device_test rsf_native_sr_fsr4_vulkan_test
        rsf_native_fg_dlss_vulkan_test rsf_native_ffx_vulkan_fixture)
    if(TARGET ${rsf_fixture_target})
        set_target_properties(${rsf_fixture_target} PROPERTIES RSF_ORCHESTRATOR_FIXTURES ON)
        add_dependencies(${rsf_fixture_target} rsf_rust_core_fixtures)
    endif()
endforeach()
# The state fixtures drive their owners through CPU doubles of the native leaves, which only the
# fixture-leaves archive selects; every other executable keeps the real leaves.
foreach(rsf_fixture_target
        rsf_native_fsr4_fg_state_test rsf_native_sr_fsr4_state_test rsf_native_fg_cpu_test rsf_xell_sleep_concurrency)
    if(TARGET ${rsf_fixture_target})
        set_target_properties(${rsf_fixture_target} PROPERTIES RSF_ORCHESTRATOR_FIXTURE_LEAVES ON)
        add_dependencies(${rsf_fixture_target} rsf_rust_core_fixture_leaves)
    endif()
endforeach()
