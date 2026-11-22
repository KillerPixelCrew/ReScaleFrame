// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif
// One owner per adapter DLL. Install after initializing MinHook. The callback must
// return without throwing and remain callable until the detour readers drain.
void rsf_xinput_facts_install_loaded(void (*callback)(void));
void rsf_xinput_facts_disable(void);
bool rsf_xinput_facts_idle(void);
bool rsf_xinput_facts_remove(void);
#ifdef __cplusplus
}
#endif
