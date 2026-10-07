/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_PREFERENCES_H
#define RSF_PREFERENCES_H
#include <stdint.h>
#include <wchar.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Windows per-user [Rendering] profile, separate from installation diagnostics. Caller supplies
   defaults and both writable outputs. Missing/out-of-range Enabled (0..1) or Quality (0..4)
   preserve that default; integer text parsing follows GetPrivateProfileIntW semantics. Reads
   are synchronous and no output/error object is allocated. */
void rsf_preferences_read(const wchar_t* path, uint32_t* enabled, uint32_t* quality);
/* Write Quality then Enabled, returning nonzero only if both Win32 writes succeed. This is not
   a transaction: failure of the second write can leave Quality persisted. Null/empty paths or
   values outside the above ranges return zero. Caller creates the parent directory. */
int rsf_preferences_write(const wchar_t* path, uint32_t enabled, uint32_t quality);
#ifdef __cplusplus
}
#endif
#endif
