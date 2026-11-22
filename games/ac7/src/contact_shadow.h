/* SPDX-License-Identifier: MIT */
#ifndef RSF_AC7_CONTACT_SHADOW_H
#define RSF_AC7_CONTACT_SHADOW_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define RSF_AC7_CONTACT_UNMATCHED 0
#define RSF_AC7_CONTACT_PATCHED 1
#define RSF_AC7_CONTACT_REFUSED 2

#ifdef __cplusplus
extern "C" {
#define RSF_AC7_CONTACT_NOEXCEPT noexcept
#else
#define RSF_AC7_CONTACT_NOEXCEPT
#endif

typedef bool (*rsf_ac7_contact_shader_fn)(void*, const uint8_t*, size_t, uint32_t);

/* Input contains the RHI prefix, followed by DXBC at offset and any engine trailer.
   On success the factory borrows the complete replacement and its DXBC CRC for the call. */
int32_t rsf_ac7_contact_shadow_correct(const uint8_t* bytes, size_t size, size_t offset,
    rsf_ac7_contact_shader_fn use_shader, void* user) RSF_AC7_CONTACT_NOEXCEPT;

#undef RSF_AC7_CONTACT_NOEXCEPT
#ifdef __cplusplus
}
#endif
#endif
