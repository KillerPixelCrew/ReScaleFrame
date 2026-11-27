/* SPDX-License-Identifier: MIT */
#pragma once

#ifdef __cplusplus
extern "C" {
#endif

/* Rust capture owner lifetime. fg12_platform.c runs these at module attach and detach. */
void rsf_fg12_capture_attach(void);
void rsf_fg12_capture_detach(void);

#ifdef __cplusplus
}
#endif
