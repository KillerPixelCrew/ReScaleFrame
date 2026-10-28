// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <stdint.h>
#include <stddef.h>
#if defined(RSF_GAME_UNITY_HOST_BUILD)
#define RSF_UNITY_HOST_API __declspec(dllexport)
#else
#define RSF_UNITY_HOST_API __declspec(dllimport)
#endif
#define RSF_UNITY_ENGINE_VERSION "6000.3"
#define RSF_UNITY_PROJECT_ID "57a42c7e-faf0-4bda-a9f9-892870948ac1"
#ifdef __cplusplus
extern "C" {
#endif
RSF_UNITY_HOST_API uint32_t __stdcall rsf_unity_sr_start(const wchar_t* configuration_path);
RSF_UNITY_HOST_API uint32_t __stdcall rsf_unity_sr_stop(void* unused);
RSF_UNITY_HOST_API uint32_t __stdcall rsf_unity_fg_start(const wchar_t* configuration_path);
RSF_UNITY_HOST_API uint32_t __stdcall rsf_unity_start(const wchar_t* configuration_path);
#ifdef __cplusplus
}
#endif
