// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif
// Returns the GameSDK input kind while the message's WM_INPUT handle is live.
uint32_t rsf_win32_input_kind(uint32_t message, intptr_t lparam);
#ifdef __cplusplus
}
#endif
