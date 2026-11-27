/* SPDX-License-Identifier: MIT */
#pragma once
#include <windows.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif
/* Writes 32 digest bytes on success. Sharing is a CreateFileW sharing mask. */
uint32_t rsf_common_sha256_file(const wchar_t* path, uint32_t sharing, unsigned char* digest);
/* Writes 64 lowercase hexadecimal bytes, without a terminator, on success. */
uint32_t rsf_common_sha256_file_hex(const wchar_t* path, char* output);
#ifdef __cplusplus
}
#endif
