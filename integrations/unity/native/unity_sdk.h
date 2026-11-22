/* SPDX-License-Identifier: MIT */
#pragma once
/* The C leaves include the Unity interface headers through here. Compiled as C, each header
   defines its interface GUIDs as external constants, so two leaves that include the same header
   in one module would collide at link time. Each leaf defines RSF_UNITY_SDK_LEAF first and its
   copies take that prefix. RSF_UNITY_GET replaces the C++ IUnityInterfaces::Get<T>() and calls
   the same GetInterface entry. */
#ifndef RSF_UNITY_SDK_LEAF
#error "Define RSF_UNITY_SDK_LEAF before including unity_sdk.h"
#endif
#ifdef __cplusplus
#error "unity_sdk.h is for the C11 Unity leaves"
#endif

#define RSF_UNITY_GUID_JOIN_(leaf, name) leaf##_##name
#define RSF_UNITY_GUID_JOIN(leaf, name) RSF_UNITY_GUID_JOIN_(leaf, name)
#define IUnityGraphics_GUID RSF_UNITY_GUID_JOIN(RSF_UNITY_SDK_LEAF, graphics_guid)
#define IUnityGraphicsD3D11_GUID RSF_UNITY_GUID_JOIN(RSF_UNITY_SDK_LEAF, d3d11_guid)
#define IUnityGraphicsD3D12_GUID RSF_UNITY_GUID_JOIN(RSF_UNITY_SDK_LEAF, d3d12_guid)
#define IUnityGraphicsD3D12v2_GUID RSF_UNITY_GUID_JOIN(RSF_UNITY_SDK_LEAF, d3d12v2_guid)
#define IUnityGraphicsD3D12v3_GUID RSF_UNITY_GUID_JOIN(RSF_UNITY_SDK_LEAF, d3d12v3_guid)
#define IUnityGraphicsD3D12v4_GUID RSF_UNITY_GUID_JOIN(RSF_UNITY_SDK_LEAF, d3d12v4_guid)
#define IUnityGraphicsD3D12v5_GUID RSF_UNITY_GUID_JOIN(RSF_UNITY_SDK_LEAF, d3d12v5_guid)
#define IUnityGraphicsD3D12v6_GUID RSF_UNITY_GUID_JOIN(RSF_UNITY_SDK_LEAF, d3d12v6_guid)
#define IUnityGraphicsD3D12v7_GUID RSF_UNITY_GUID_JOIN(RSF_UNITY_SDK_LEAF, d3d12v7_guid)
#define IUnityGraphicsVulkan_GUID RSF_UNITY_GUID_JOIN(RSF_UNITY_SDK_LEAF, vulkan_guid)
#define IUnityGraphicsVulkanV2_GUID RSF_UNITY_GUID_JOIN(RSF_UNITY_SDK_LEAF, vulkan_v2_guid)
/* The Vulkan header defines this null subresource the same way. */
#define UnityVulkanWholeImage RSF_UNITY_GUID_JOIN(RSF_UNITY_SDK_LEAF, vulkan_whole_image)

#include <IUnityInterface.h>
#include <IUnityGraphics.h>

#define RSF_UNITY_GET(interfaces, TYPE) ((TYPE*)(interfaces)->GetInterface(UNITY_GET_INTERFACE_GUID(TYPE)))
