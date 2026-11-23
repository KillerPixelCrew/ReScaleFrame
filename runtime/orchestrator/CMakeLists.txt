# The orchestrator's code lives in an object library so it can be linked both into the runtime DLL
# and directly into the research proxy, which is what actually gets loaded into the game today.
# Compiling it twice, or requiring the runtime DLL beside the game executable before anything works,
# would both be worse.
# The capture readers (rsf_input_dump*) are Rust exports in the Runtime archive; this target carries
# their headers and that archive.
add_library(rsf_frame_dump INTERFACE)
target_include_directories(rsf_frame_dump INTERFACE include "${CMAKE_SOURCE_DIR}/runtime/backends/dlss/include")
target_link_libraries(rsf_frame_dump INTERFACE ReScaleFrame::Graphics ReScaleFrame::Contract ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon)
add_library(ReScaleFrame::FrameDump ALIAS rsf_frame_dump)

# The CPU fact dispatcher is implemented in the Rust core; this target carries its headers and archive.
add_library(rsf_native_cpu INTERFACE)
target_include_directories(rsf_native_cpu INTERFACE include)
target_link_libraries(rsf_native_cpu INTERFACE ReScaleFrame::GameSDK)

# The native leaves the Rust core calls back into. Everything that links the Rust core links them,
# so Runtime tests and embedded hosts resolve the same functions the DLL does.
add_library(rsf_runtime_leaves STATIC
    src/fg12_platform.c
    src/native_composition_unload.c
    src/render_host_platform.c
    src/d3d11_host_foreign.c
    ../graphics/src/observer_exception_guards.c
)
target_include_directories(rsf_runtime_leaves PUBLIC include)
target_compile_definitions(rsf_runtime_leaves PUBLIC RSF_RUNTIME_BUILD PRIVATE RSF_STREAMLINE_HOST_BUILD)
target_link_libraries(rsf_runtime_leaves PUBLIC rsf_native_cpu ReScaleFrame::GameSDK ReScaleFrame::DLSS
    ReScaleFrame::Graphics ReScaleFrame::FSR ReScaleFrame::XeSS ReScaleFrame::Presentation
    ReScaleFrame::FrameDump PRIVATE rsf_build_options d3dcompiler bcrypt)

add_library(rsf_orchestrator_core OBJECT
    src/rust_exports.c
)
target_include_directories(rsf_orchestrator_core PUBLIC include)
target_compile_definitions(rsf_orchestrator_core PUBLIC RSF_RUNTIME_BUILD)
target_link_libraries(rsf_orchestrator_core PUBLIC rsf_native_cpu rsf_runtime_leaves)
# The corrected Streamline Vulkan runtime, verified against its receipt. Its interposer digest reaches
# rsf-streamline-vk through the Cargo build environment, which admits only that file.
set(RSF_STREAMLINE_VK_RUNTIME "${CMAKE_SOURCE_DIR}/.local/godot/vulkan/host/artifacts/streamline-production-selected-device-reflex/runtime" CACHE PATH "Corrected Streamline Production runtime")
set(RSF_STREAMLINE_VK_DIGEST)
set(rsf_orchestrator_env)
if(RSF_HAVE_VULKAN AND RSF_HAVE_STREAMLINE AND EXISTS "${RSF_STREAMLINE_VK_RUNTIME}/streamline-sdk-build.json")
    include("${CMAKE_SOURCE_DIR}/runtime/vulkan/streamline/verify-receipt.cmake")
    rsf_streamline_receipt("${RSF_STREAMLINE_VK_RUNTIME}" RSF_STREAMLINE_VK_DIGEST)
    set(rsf_orchestrator_env ENV "RSF_CORRECTED_STREAMLINE_SHA256=${RSF_STREAMLINE_VK_DIGEST}")
endif()# The vendor headers cmake/vendor.cmake found select the D3D12 FidelityFX and XeSS providers, the
# XeSS Vulkan SR table, AMD Anti-Lag 2 for Vulkan and, with a verified runtime, Streamline Vulkan.
set(rsf_orchestrator_features)
if(RSF_HAVE_VULKAN AND RSF_HAVE_XESS_VK)
    list(APPEND rsf_orchestrator_features --features xess-vk)
endif()
if(RSF_HAVE_VULKAN)
    list(APPEND rsf_orchestrator_features --features antilag-vk)
endif()
if(RSF_HAVE_VULKAN AND RSF_STREAMLINE_VK_DIGEST)
    list(APPEND rsf_orchestrator_features --features streamline-vk)
endif()
# The FidelityFX Vulkan SR table behind rsf_srvk_* (vulkan_sr.rs), as RSF_VULKAN_SR_ENABLED.
if(RSF_HAVE_VULKAN AND RSF_HAVE_FFX_VK)
    list(APPEND rsf_orchestrator_features --features ffx-vk)
endif()
# The Vulkan FSR 4 and XeSS FG providers of vulkan_fg.rs, as RSF_HAVE_FSR4_FG_VK and RSF_HAVE_XESS_FG_VK.
if(RSF_HAVE_VULKAN AND RSF_HAVE_FFX_DX12_FG)
    list(APPEND rsf_orchestrator_features --features fsr4-fg-vk)
endif()
if(RSF_HAVE_VULKAN AND RSF_HAVE_XESS_FG)
    list(APPEND rsf_orchestrator_features --features xess-fg-vk)
endif()
# The Vulkan FSR 4 SR provider behind rsf_srvk_fsr4_* (vulkan_sr_split.rs), as RSF_VULKAN_FSR4_ENABLED.
if(RSF_HAVE_VULKAN AND RSF_HAVE_FFX_DX12_SR)
    list(APPEND rsf_orchestrator_features --features fsr4-vk)
endif()
foreach(rsf_vendor "FFX;ffx" "FFX_FG;ffx-fg" "XESS;xess" "XESS_FG;xess-fg")
    list(GET rsf_vendor 0 rsf_vendor_flag)
    list(GET rsf_vendor 1 rsf_vendor_feature)
    if(RSF_HAVE_${rsf_vendor_flag})
        list(APPEND rsf_orchestrator_features --features ${rsf_vendor_feature})
    endif()
endforeach()
rsf_add_rust_static(rsf_orchestrator_rust rsf_rust_core rsf-orchestrator rsf_orchestrator ${rsf_orchestrator_env}
    ${rsf_orchestrator_features})
if(BUILD_TESTING)
    # The same crate with the test-fixtures feature adds the fixture exports the ctest executables call. A test
    # target that sets RSF_ORCHESTRATOR_FIXTURES links this archive wherever the shipping one is named, so the
    # Runtime and the carriers never contain a fixture.
    rsf_add_rust_static(rsf_orchestrator_fixture_rust rsf_rust_core_fixtures rsf-orchestrator rsf_orchestrator
        ${rsf_orchestrator_env} --features test-fixtures ${rsf_orchestrator_features})
    get_target_property(rsf_orchestrator_libs rsf_orchestrator_rust INTERFACE_LINK_LIBRARIES)
    get_target_property(rsf_orchestrator_fixture_libs rsf_orchestrator_fixture_rust INTERFACE_LINK_LIBRARIES)
    list(POP_FRONT rsf_orchestrator_libs rsf_orchestrator_archive)
    list(POP_FRONT rsf_orchestrator_fixture_libs rsf_orchestrator_fixture_archive)
    set_property(TARGET rsf_orchestrator_rust PROPERTY INTERFACE_LINK_LIBRARIES
        "$<IF:$<BOOL:$<TARGET_PROPERTY:RSF_ORCHESTRATOR_FIXTURES>>,${rsf_orchestrator_fixture_archive},${rsf_orchestrator_archive}>"
        ${rsf_orchestrator_libs})
endif()
# version: the provider version reads in mfg_provider.rs (forwarded by the dinput8 proxy itself).
target_link_libraries(rsf_orchestrator_rust INTERFACE minhook rsf_runtime_leaves version)
target_link_libraries(rsf_orchestrator_core PUBLIC rsf_orchestrator_rust)
target_link_libraries(rsf_native_cpu INTERFACE rsf_orchestrator_rust)
target_link_libraries(rsf_frame_dump INTERFACE rsf_orchestrator_rust)
target_compile_definitions(rsf_orchestrator_core PRIVATE RSF_STREAMLINE_HOST_BUILD)
# Public because the orchestrator's own headers name a backend's frame structure and the decode
# pass's parameters.
target_link_libraries(rsf_orchestrator_core PUBLIC ReScaleFrame::DLSS ReScaleFrame::Graphics ReScaleFrame::FSR ReScaleFrame::XeSS ReScaleFrame::Presentation)
target_link_libraries(rsf_orchestrator_core PUBLIC ReScaleFrame::GameSDK PRIVATE rsf_build_options)
target_link_libraries(rsf_orchestrator_core PRIVATE d3dcompiler bcrypt)
target_link_libraries(rsf_orchestrator_core PUBLIC ReScaleFrame::FrameDump)
add_library(ReScaleFrame::OrchestratorCore ALIAS rsf_orchestrator_core)

add_library(rsf_native_vulkan STATIC src/native_xess_vulkan.c src/native_vulkan_exports.c)
if(RSF_HAVE_VULKAN)
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
endif()
target_include_directories(rsf_native_vulkan PUBLIC include)
target_compile_definitions(rsf_native_vulkan PUBLIC RSF_RUNTIME_BUILD)
target_link_libraries(rsf_native_vulkan PUBLIC ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::FrameDump rsf_native_cpu PRIVATE rsf_build_options)
if(RSF_HAVE_VULKAN AND RSF_HAVE_FFX_VK)
    # native_sr_vulkan.c only checks the configured SDK headers' layouts, so it needs them.
    target_sources(rsf_native_vulkan PRIVATE src/native_sr_vulkan.c)
    target_compile_definitions(rsf_native_vulkan PRIVATE RSF_HAVE_FFX_VK=1)
    target_link_libraries(rsf_native_vulkan PRIVATE rsf_orchestrator_rust)
    target_compile_definitions(rsf_native_vulkan PUBLIC RSF_VULKAN_SR_ENABLED=1)
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_FFX_VK_INCLUDE}")
endif()
if(RSF_HAVE_VULKAN AND (RSF_HAVE_XESS_FG OR RSF_HAVE_FFX_FG OR RSF_HAVE_FFX_DX12_SR))
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_native_vulkan PRIVATE d3d12 dxgi)
endif()
if(RSF_HAVE_VULKAN AND RSF_HAVE_XESS_VK)
    target_compile_definitions(rsf_native_vulkan PRIVATE RSF_HAVE_XESS_VK=1)
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_XESS_INCLUDE}")
endif()
if(RSF_HAVE_VULKAN AND RSF_HAVE_XESS_FG)
    target_sources(rsf_native_vulkan PRIVATE src/native_fg_xess_vulkan.c src/native_fg_xess_vulkan_present.c ../vulkan/xess-fg/bridge.c)
    target_compile_definitions(rsf_native_vulkan PRIVATE RSF_HAVE_XESS_FG_VK=1 RSF_XESS_BRIDGE_RUNTIME_D3D12=1 PUBLIC RSF_VULKAN_XESS_FG_ENABLED=1)
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_XESS_INCLUDE}")
    # dxguid defines the DXGI interface IDs that bridge.c passes by address.
    target_link_libraries(rsf_native_vulkan PRIVATE rsf_orchestrator_rust d3d12 dxgi dxguid bcrypt)
endif()
if(RSF_HAVE_VULKAN AND RSF_HAVE_FFX_DX12_SR)
    target_compile_definitions(rsf_native_vulkan PUBLIC RSF_VULKAN_FSR4_ENABLED=1)
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    # The FSR 4 SR owner, its bridge and their exports are Rust (vulkan_fsr4_sr.rs, rsf-fsr4-compat).
    target_link_libraries(rsf_native_vulkan PRIVATE ReScaleFrame::FSR rsf_orchestrator_rust bcrypt d3d12 dxgi)
endif()
if(RSF_HAVE_VULKAN AND RSF_HAVE_FFX_DX12_FG)
    # The FSR 4 FG bridge, its platform table and exports are Rust (vulkan_fsr4_fg.rs, runtime/vulkan/fsr4-fg).
    target_compile_definitions(rsf_native_vulkan PRIVATE RSF_HAVE_FSR4_FG_VK=1 PUBLIC RSF_VULKAN_FSR4_FG_ENABLED=1)
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_FFX_INCLUDE}" "${RSF_FFX_INCLUDE}/../../framegeneration/include")
    target_link_libraries(rsf_native_vulkan PRIVATE rsf_orchestrator_rust d3d12 dxgi)
endif()
if(RSF_STREAMLINE_VK_DIGEST)
    target_compile_definitions(rsf_native_vulkan PRIVATE RSF_HAVE_STREAMLINE_VK=1 RSF_STREAMLINE_VULKAN_STATIC=1)
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_STREAMLINE_INCLUDE}")
    # The interposer lifecycle is Rust (rsf-streamline-vk, inside the Runtime archive); the standalone
    # bridge links the same crate.
    target_link_libraries(rsf_native_vulkan PRIVATE rsf_orchestrator_rust wintrust advapi32)
endif()
add_library(ReScaleFrame::NativeVulkan ALIAS rsf_native_vulkan)
if(RSF_FRAME_DUMP_VULKAN_INCLUDE)
    # The C file only carries export directives. Link it as an object in each
    # consumer so the linker sees the exports before extracting the Rust archive.
    target_sources(rsf_native_vulkan INTERFACE "${CMAKE_CURRENT_SOURCE_DIR}/src/input_dump_vulkan_exports.c")
    target_include_directories(rsf_native_vulkan SYSTEM PUBLIC "${RSF_FRAME_DUMP_VULKAN_INCLUDE}")
    target_compile_definitions(rsf_native_vulkan PUBLIC RSF_FRAME_DUMP_VULKAN=1)
endif()

add_library(rsf_orchestrator SHARED $<TARGET_OBJECTS:rsf_orchestrator_core>)
target_link_libraries(rsf_orchestrator PRIVATE rsf_orchestrator_core)
# Consumers link the DLL's exports. They get the core's headers and definitions, not its static
# archives, which would define the same exports a second time.
target_include_directories(rsf_orchestrator INTERFACE
    $<TARGET_PROPERTY:rsf_orchestrator_core,INTERFACE_INCLUDE_DIRECTORIES>)
target_compile_definitions(rsf_orchestrator INTERFACE
    $<TARGET_PROPERTY:rsf_orchestrator_core,INTERFACE_COMPILE_DEFINITIONS>)
# Native Vulkan services are public C entry points used by external engines.
# Keep every service object in the DLL even when no embedded host references it.
target_link_libraries(rsf_orchestrator PRIVATE "$<LINK_LIBRARY:WHOLE_ARCHIVE,rsf_native_vulkan>")
target_link_libraries(rsf_orchestrator PRIVATE rsf_build_options)
set_target_properties(rsf_orchestrator PROPERTIES OUTPUT_NAME "ReScaleFrame.Runtime")
target_sources(rsf_orchestrator PRIVATE src/d3d11_present_api.c src/dll_exports.c)
target_sources(rsf_orchestrator PRIVATE src/d3d11_host_exports.c)
add_subdirectory(../shader_patch "${CMAKE_CURRENT_BINARY_DIR}/shader_patch")
target_include_directories(rsf_orchestrator PUBLIC "${CMAKE_SOURCE_DIR}/runtime/shader_patch/include")
target_link_libraries(rsf_orchestrator PRIVATE rsf_shader_patch)
if(MSVC)
    target_link_options(rsf_orchestrator PRIVATE "LINKER:/EXPORT:rsf_get_shader_patch_api")
else()
    target_sources(rsf_orchestrator PRIVATE "${CMAKE_SOURCE_DIR}/runtime/shader_patch/shader_patch.def")
    target_link_options(rsf_orchestrator PRIVATE "LINKER:--undefined,rsf_get_shader_patch_api")
endif()

# Anti-Lag 2 driver calls are Rust (src/latency_antilag_sdk.rs); no SDK headers are needed.
