/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>

/* The renderer Unity runs on, as rsf_unity_package_sdk_renderer reports it to the Rust bridge. */
#define RSF_UNITY_BRIDGE_NONE 0u
#define RSF_UNITY_BRIDGE_D3D11 1u
#define RSF_UNITY_BRIDGE_D3D12 2u
#define RSF_UNITY_BRIDGE_VULKAN 3u
