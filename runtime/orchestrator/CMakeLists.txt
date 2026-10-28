# The orchestrator's code lives in an object library so it can be linked both into the runtime DLL
# and directly into the research proxy, which is what actually gets loaded into the game today.
# Compiling it twice, or requiring the runtime DLL beside the game executable before anything works,
# would both be worse.
add_library(rsf_frame_dump STATIC src/input_dump.cpp)
target_include_directories(rsf_frame_dump PUBLIC include "${CMAKE_SOURCE_DIR}/runtime/backends/dlss/include")
target_compile_definitions(rsf_frame_dump PRIVATE RSF_RUNTIME_BUILD)
target_link_libraries(rsf_frame_dump PUBLIC ReScaleFrame::Graphics ReScaleFrame::Contract ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon PRIVATE rsf_build_options)
add_library(ReScaleFrame::FrameDump ALIAS rsf_frame_dump)
# Readback helpers in the backend libraries share the same capture owner as the host.
foreach(rsf_dump_backend rsf_dlss rsf_fsr rsf_xess)
    target_link_libraries(${rsf_dump_backend} PRIVATE ReScaleFrame::FrameDump)
    target_compile_definitions(${rsf_dump_backend} PRIVATE RSF_FRAME_DUMP_STATIC)
endforeach()

add_library(rsf_orchestrator_core OBJECT
    src/runtime.cpp
    src/plugin_session.cpp
    src/native_scene.cpp
    src/native_window.cpp
    src/native_cpu.cpp
    src/native_fg.cpp
    src/native_fg_d3d12.cpp
    src/native_composition.cpp
    src/native_sr.cpp
    src/native_translucency.cpp
    src/native_sr_d3d12.cpp
    src/gl_pipeline.cpp
    src/gpu_policy.cpp
    src/callback_render_host.cpp
    src/render_host.cpp
    src/overlay_d3d12.cpp
    src/overlay_native12.cpp
    ../../loader/proxy/src/overlay_host.cpp
    src/native_regions.cpp
    src/frame_sequencer.cpp
    src/render_links.cpp
    src/fg_session.cpp
    src/fg_choice.cpp
    src/fg_providers.cpp
    src/fg_leases.cpp
    src/sr_session.cpp
    src/sr_bridge.cpp
    src/sr_legacy_adapter.cpp
    src/frame_assembly.cpp
    src/dlss_pipeline.cpp)
target_include_directories(rsf_orchestrator_core PUBLIC include)
target_compile_definitions(rsf_orchestrator_core PUBLIC RSF_RUNTIME_BUILD)
target_compile_definitions(rsf_orchestrator_core PRIVATE RSF_STREAMLINE_HOST_BUILD)
# Public because the orchestrator's own headers name a backend's frame structure and the decode
# pass's parameters.
target_link_libraries(rsf_orchestrator_core PUBLIC ReScaleFrame::DLSS ReScaleFrame::Graphics ReScaleFrame::FSR ReScaleFrame::XeSS ReScaleFrame::Presentation)
target_link_libraries(rsf_orchestrator_core PUBLIC ReScaleFrame::GameSDK PRIVATE rsf_build_options)
target_link_libraries(rsf_orchestrator_core PRIVATE d3dcompiler bcrypt)
target_link_libraries(rsf_orchestrator_core PUBLIC ReScaleFrame::FrameDump)
add_library(ReScaleFrame::OrchestratorCore ALIAS rsf_orchestrator_core)

set(RSF_VULKAN_INCLUDE "${CMAKE_SOURCE_DIR}/references/godot/engine/thirdparty/vulkan/include" CACHE PATH "Vulkan headers for native Vulkan services")
set(RSF_FFX_VK_INCLUDE "${CMAKE_SOURCE_DIR}/references/godot/fidelityfx-vulkan/ffx-api/include" CACHE PATH "Pinned FidelityFX Vulkan API headers")
add_library(rsf_native_vulkan STATIC src/native_sr_vulkan.cpp src/native_dlss_vulkan.cpp src/native_xess_vulkan.cpp src/native_fg_vulkan.cpp src/native_fg_vulkan_dispatch.cpp src/native_fg_dlss_vulkan.cpp src/native_fg_dlss_vulkan_capture.cpp src/native_latency_vulkan.cpp)
target_include_directories(rsf_native_vulkan PUBLIC include)
target_compile_definitions(rsf_native_vulkan PUBLIC RSF_RUNTIME_BUILD)
target_link_libraries(rsf_native_vulkan PUBLIC ReScaleFrame::GameSDK ReScaleFrame::Contract ReScaleFrame::FrameDump PRIVATE rsf_build_options)
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h" AND EXISTS "${RSF_FFX_VK_INCLUDE}/ffx_api/vk/ffx_api_vk.h")
    target_compile_definitions(rsf_native_vulkan PRIVATE RSF_HAVE_FFX_VK=1)
    target_compile_definitions(rsf_native_vulkan PUBLIC RSF_VULKAN_SR_ENABLED=1)
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_FFX_VK_INCLUDE}")
endif()
set(RSF_XESS_VK_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/xess/inc" CACHE PATH "XeSS Vulkan API headers")
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h" AND
   ((EXISTS "${RSF_XESS_VK_INCLUDE}/xess_fg/xefg_swapchain_d3d12.h" AND EXISTS "${RSF_XESS_VK_INCLUDE}/xell/xell_d3d12.h") OR
    EXISTS "${RSF_FFX_INCLUDE}/../../framegeneration/include/ffx_framegeneration.h"))
    target_sources(rsf_native_vulkan PRIVATE src/native_dx12_vulkan_interop.cpp)
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}")
    target_link_libraries(rsf_native_vulkan PRIVATE d3d12 dxgi)
endif()
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h" AND EXISTS "${RSF_XESS_VK_INCLUDE}/xess/xess_vk.h")
    target_compile_definitions(rsf_native_vulkan PRIVATE RSF_HAVE_XESS_VK=1)
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_XESS_VK_INCLUDE}")
endif()
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h" AND EXISTS "${RSF_XESS_VK_INCLUDE}/xess_fg/xefg_swapchain_d3d12.h" AND EXISTS "${RSF_XESS_VK_INCLUDE}/xell/xell_d3d12.h")
    target_sources(rsf_native_vulkan PRIVATE src/native_fg_xess_vulkan.cpp src/native_fg_xess_vulkan_present.cpp ../vulkan/xess-fg/bridge.cpp)
    target_compile_definitions(rsf_native_vulkan PRIVATE RSF_HAVE_XESS_FG_VK=1 PUBLIC RSF_VULKAN_XESS_FG_ENABLED=1)
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_XESS_VK_INCLUDE}")
    target_link_libraries(rsf_native_vulkan PRIVATE d3d12 dxgi bcrypt)
endif()
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h" AND EXISTS "${RSF_FFX_INCLUDE}/dx12/ffx_api_dx12.h" AND EXISTS "${RSF_FFX_INCLUDE}/../../upscalers/include/ffx_upscale.h")
    target_sources(rsf_native_vulkan PRIVATE src/native_sr_fsr4_vulkan.cpp ../vulkan/fsr4/bridge.cpp)
    target_compile_definitions(rsf_native_vulkan PUBLIC RSF_VULKAN_FSR4_ENABLED=1)
    target_include_directories(rsf_native_vulkan PRIVATE ../backends/fsr/src)
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_FFX_INCLUDE}/../..")
    target_link_libraries(rsf_native_vulkan PRIVATE ReScaleFrame::FSR bcrypt)
endif()
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h" AND EXISTS "${RSF_FFX_INCLUDE}/dx12/ffx_api_dx12.h" AND EXISTS "${RSF_FFX_INCLUDE}/../../framegeneration/include/ffx_framegeneration.h")
    target_sources(rsf_native_vulkan PRIVATE src/native_fg_fsr4_vulkan.cpp ../vulkan/fsr4-fg/bridge.cpp)
    target_compile_definitions(rsf_native_vulkan PRIVATE RSF_HAVE_FSR4_FG_VK=1 PUBLIC RSF_VULKAN_FSR4_FG_ENABLED=1)
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_FFX_INCLUDE}" "${RSF_FFX_INCLUDE}/../../framegeneration/include")
    target_link_libraries(rsf_native_vulkan PRIVATE d3d12 dxgi)
endif()
set(RSF_STREAMLINE_VK_RUNTIME "${CMAKE_SOURCE_DIR}/.local/godot/vulkan/host/artifacts/streamline-production-selected-device-reflex/runtime" CACHE PATH "Corrected Streamline Production runtime")
if(EXISTS "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h" AND EXISTS "${RSF_STREAMLINE_INCLUDE}/sl.h" AND EXISTS "${RSF_STREAMLINE_VK_RUNTIME}/streamline-sdk-build.json")
    file(READ "${RSF_STREAMLINE_VK_RUNTIME}/streamline-sdk-build.json" rsf_sl_receipt)
    string(JSON rsf_sl_revision GET "${rsf_sl_receipt}" sdkCommit)
    if(NOT rsf_sl_revision STREQUAL "2122257e0fce486f91b385aa63b9a09b0a34b363")
        message(FATAL_ERROR "Unexpected corrected Streamline source revision")
    endif()
    foreach(rsf_sl_input "patchSha256|sdk-fixes/prepare.py" "dispatchSha256|sdk-fixes/ngx-vulkan-dispatch.h" "recipeSha256|build-sdk.ps1" "presentCaptureSha256|present-capture.h" "selectedDevicePatchSha256|sdk-fixes/selected_device.py" "selectedDeviceConfigSha256|sdk-fixes/selected-device-config.h")
        string(REPLACE "|" ";" rsf_sl_fields "${rsf_sl_input}")
        list(GET rsf_sl_fields 0 rsf_sl_field)
        list(GET rsf_sl_fields 1 rsf_sl_file)
        file(SHA256 "${CMAKE_SOURCE_DIR}/runtime/vulkan/streamline/${rsf_sl_file}" rsf_sl_hash)
        string(JSON rsf_sl_expected GET "${rsf_sl_receipt}" "${rsf_sl_field}")
        string(TOLOWER "${rsf_sl_expected}" rsf_sl_expected)
        if(NOT rsf_sl_hash STREQUAL rsf_sl_expected)
            message(FATAL_ERROR "Corrected Streamline receipt differs from ${rsf_sl_file}")
        endif()
    endforeach()
    foreach(rsf_sl_binary sl.interposer.dll sl.common.dll)
        file(SHA256 "${RSF_STREAMLINE_VK_RUNTIME}/${rsf_sl_binary}" rsf_sl_hash)
        string(JSON rsf_sl_expected GET "${rsf_sl_receipt}" binaries "${rsf_sl_binary}")
        string(TOLOWER "${rsf_sl_expected}" rsf_sl_expected)
        if(NOT rsf_sl_hash STREQUAL rsf_sl_expected)
            message(FATAL_ERROR "Corrected Streamline binary differs from receipt: ${rsf_sl_binary}")
        endif()
    endforeach()
    file(SHA256 "${RSF_STREAMLINE_VK_RUNTIME}/sl.interposer.dll" rsf_sl_hash)
    string(REGEX REPLACE "([0-9a-f][0-9a-f])" "0x\\1," rsf_sl_bytes "${rsf_sl_hash}")
    target_sources(rsf_native_vulkan PRIVATE ../vulkan/streamline/bridge.cpp)
    target_compile_definitions(rsf_native_vulkan PRIVATE RSF_HAVE_STREAMLINE_VK=1 RSF_STREAMLINE_VULKAN_STATIC=1 "VULKANSTORY_CORRECTED_STREAMLINE_SHA256=${rsf_sl_bytes}")
    target_include_directories(rsf_native_vulkan SYSTEM PRIVATE "${RSF_VULKAN_INCLUDE}" "${RSF_STREAMLINE_INCLUDE}")
    target_link_libraries(rsf_native_vulkan PRIVATE wintrust advapi32)
endif()
add_library(ReScaleFrame::NativeVulkan ALIAS rsf_native_vulkan)
find_path(RSF_FRAME_DUMP_VULKAN_INCLUDE vulkan/vulkan.h
    HINTS "${RSF_VULKAN_INCLUDE}" "$ENV{VULKAN_SDK}/Include")
if(RSF_FRAME_DUMP_VULKAN_INCLUDE)
    target_sources(rsf_native_vulkan PRIVATE src/input_dump_vulkan.cpp)
    target_include_directories(rsf_native_vulkan SYSTEM PUBLIC "${RSF_FRAME_DUMP_VULKAN_INCLUDE}")
    target_compile_definitions(rsf_native_vulkan PUBLIC RSF_FRAME_DUMP_VULKAN=1)
endif()

add_library(rsf_orchestrator SHARED $<TARGET_OBJECTS:rsf_orchestrator_core>)
target_link_libraries(rsf_orchestrator PUBLIC rsf_orchestrator_core)
target_link_libraries(rsf_orchestrator PRIVATE rsf_build_options)
set_target_properties(rsf_orchestrator PROPERTIES OUTPUT_NAME "ReScaleFrame.Runtime")
