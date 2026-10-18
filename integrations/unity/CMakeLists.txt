if(EXISTS "${RSF_UNITY_NATIVE_INCLUDE}/IUnityGraphicsD3D12.h")
    find_package(Vulkan REQUIRED)
    add_library(rsf_unity_package_bridge OBJECT ../../games/unity-mono/src/bridge.cpp)
    target_include_directories(rsf_unity_package_bridge PRIVATE ../../games/unity-mono/include)
    target_include_directories(rsf_unity_package_bridge SYSTEM PRIVATE "${RSF_UNITY_NATIVE_INCLUDE}")
    target_compile_definitions(rsf_unity_package_bridge PRIVATE RSF_HAVE_UNITY_NATIVE=1
        UnityPluginLoad=rsf_unity_d3d12_load UnityPluginUnload=rsf_unity_d3d12_unload
        rsf_unity_bridge_prepare=rsf_unity_d3d12_prepare rsf_unity_bridge_activate=rsf_unity_d3d12_activate
        rsf_unity_bridge_drained=rsf_unity_d3d12_drained rsf_unity_bridge_release=rsf_unity_d3d12_release
        rsf_unity_bridge_managed_state=rsf_unity_d3d12_managed_state
        rsf_unity_get_render_event=rsf_unity_d3d12_get_render_event rsf_unity_get_event_id=rsf_unity_d3d12_get_event_id
        rsf_unity_set_engine_spatial=rsf_unity_d3d12_set_engine_spatial)
    target_link_libraries(rsf_unity_package_bridge PRIVATE ReScaleFrame::GameSDK ReScaleFrame::RuntimeCommon rsf_build_options)
    add_library(rsf_unity_package SHARED native/package.cpp native/bridge.cpp native/preview.cpp native/vulkan_bridge.cpp $<TARGET_OBJECTS:rsf_unity_package_bridge>)
    target_compile_definitions(rsf_unity_package PRIVATE VK_USE_PLATFORM_WIN32_KHR RSF_STREAMLINE_HOST_BUILD)
    target_include_directories(rsf_unity_package SYSTEM PRIVATE "${Vulkan_INCLUDE_DIRS}")
    target_link_libraries(rsf_unity_package PRIVATE Vulkan::Vulkan)
    if(MSVC)
        target_link_libraries(rsf_unity_package PRIVATE delayimp)
        target_link_options(rsf_unity_package PRIVATE /DELAYLOAD:vulkan-1.dll)
    endif()
    target_include_directories(rsf_unity_package PRIVATE native ../../games/unity-mono/include ../../games/unity-mono/src)
    target_include_directories(rsf_unity_package SYSTEM PRIVATE "${RSF_UNITY_NATIVE_INCLUDE}")
    target_link_libraries(rsf_unity_package PRIVATE rsf_orchestrator_core rsf_build_options d3d12 d3d11)
    set_target_properties(rsf_unity_package PROPERTIES OUTPUT_NAME "ReScaleFrame.Unity.Native")
endif()
