/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_PREFERENCES_H
#define RSF_PREFERENCES_H
#include <stdint.h>
#include <wchar.h>
#ifdef __cplusplus
extern "C" {
#endif
/* A user-owned Windows profile file, separate from the installation's diagnostic settings.
   Caller supplies defaults; missing or invalid values leave them unchanged. */
void rsf_preferences_read(const wchar_t* path, uint32_t* enabled, uint32_t* quality);
int rsf_preferences_write(const wchar_t* path, uint32_t enabled, uint32_t quality);
#ifdef __cplusplus
}
#endif
#endif
