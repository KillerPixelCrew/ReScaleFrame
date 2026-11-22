/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Copies live AC7 engine bytes under the platform's guarded memory read. */
bool rsf_ac7_copy_memory(void* output, const void* input, size_t size);
bool rsf_ac7_read_memory(void* output, uintptr_t base, size_t offset, size_t size);

#ifdef __cplusplus
}
#endif
