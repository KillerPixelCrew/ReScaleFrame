# Vendor SDK header trees, each located once. They are fetched into untracked directories, and a
# missing tree switches its backend off (the C providers answer NOT_COMPILED), so a checkout
# without them still configures, builds and tests. Override a path with -D<name>=...; the RSF_HAVE_*
# flags below are what the rest of the build tests.
set(RSF_STREAMLINE_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/streamline/include" CACHE PATH
    "Directory holding Streamline's sl.h")
set(RSF_FFX_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/fidelityfx/Kits/FidelityFX/api/include" CACHE PATH
    "Directory holding FidelityFX's ffx_api.h")
set(RSF_XESS_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/xess/inc" CACHE PATH
    "Directory holding XeSS's xess/xess.h and the Vulkan and frame generation headers beside it")
set(RSF_RENDERDOC_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/renderdoc" CACHE PATH
    "Directory holding RenderDoc's renderdoc_app.h")
set(RSF_UNITY_NATIVE_INCLUDE "${CMAKE_SOURCE_DIR}/vendor/unity-native/include" CACHE PATH
    "Unity native-plugin headers; untracked, see docs/dependencies.md")
set(RSF_VULKAN_INCLUDE "${CMAKE_SOURCE_DIR}/references/godot/engine/thirdparty/vulkan/include" CACHE PATH
    "Vulkan headers for native Vulkan services")
set(RSF_FFX_VK_INCLUDE "${CMAKE_SOURCE_DIR}/references/godot/fidelityfx-vulkan/ffx-api/include" CACHE PATH
    "Pinned FidelityFX Vulkan API headers")

function(rsf_have name)
    set(${name} ON)
    foreach(header IN LISTS ARGN)
        if(NOT EXISTS "${header}")
            set(${name} OFF)
        endif()
    endforeach()
    set(${name} ${${name}} PARENT_SCOPE)
endfunction()

rsf_have(RSF_HAVE_VULKAN "${RSF_VULKAN_INCLUDE}/vulkan/vulkan.h")
rsf_have(RSF_HAVE_STREAMLINE "${RSF_STREAMLINE_INCLUDE}/sl.h")
rsf_have(RSF_HAVE_XESS "${RSF_XESS_INCLUDE}/xess/xess.h")
rsf_have(RSF_HAVE_XESS_VK "${RSF_XESS_INCLUDE}/xess/xess_vk.h")
rsf_have(RSF_HAVE_XESS_FG "${RSF_XESS_INCLUDE}/xess_fg/xefg_swapchain_d3d12.h" "${RSF_XESS_INCLUDE}/xell/xell_d3d12.h")
rsf_have(RSF_HAVE_FFX_VK "${RSF_FFX_VK_INCLUDE}/ffx_api/vk/ffx_api_vk.h")
rsf_have(RSF_HAVE_FFX "${RSF_FFX_INCLUDE}/ffx_api.h")
rsf_have(RSF_HAVE_FFX_FG "${RSF_FFX_INCLUDE}/../../framegeneration/include/ffx_framegeneration.h")
rsf_have(RSF_HAVE_FFX_DX12_FG "${RSF_FFX_INCLUDE}/dx12/ffx_api_dx12.h"
    "${RSF_FFX_INCLUDE}/../../framegeneration/include/ffx_framegeneration.h")
rsf_have(RSF_HAVE_FFX_DX12_SR "${RSF_FFX_INCLUDE}/dx12/ffx_api_dx12.h"
    "${RSF_FFX_INCLUDE}/../../upscalers/include/ffx_upscale.h")

# Vulkan headers for the frame dump: the pinned tree above, else the installed SDK.
find_path(RSF_FRAME_DUMP_VULKAN_INCLUDE vulkan/vulkan.h
    HINTS "${RSF_VULKAN_INCLUDE}" "$ENV{VULKAN_SDK}/Include")
