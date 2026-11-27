/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/runtime.h>
#include <rescaleframe/overlay.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_OVERLAY_SAVED_OPTIONS_ABI_VERSION 1u
#define RSF_OVERLAY_SAVED_TEXT_LIMIT 4096u

typedef struct rsf_overlay_saved_option {
    const char* label_utf8;
    const char* key_utf8;
    const uint16_t* default_text;
    uint32_t default_text_length;
    uint32_t kind, restart, max_text_length;
    int32_t default_value, minimum, maximum, step;
    uint32_t divisor;
} rsf_overlay_saved_option;

/* The callback receives the descriptor's index and a NUL-terminated UTF-16
   value (decimal for numeric rows). The value is borrowed for the call.
   Return RSF_OK after applying, or refuse without changing live state.
   Restart rows are persisted without calling apply. */
typedef int32_t (*rsf_overlay_saved_apply)(void*, uint32_t, const uint16_t*, uint32_t);

typedef struct rsf_overlay_saved_setup {
    uint32_t struct_size, abi_version;
    const wchar_t* configuration_path;
    const wchar_t* configuration_section;
    const rsf_overlay_saved_option* options;
    uint32_t option_count;
    rsf_overlay_saved_apply apply;
    void* user;
} rsf_overlay_saved_setup;

/* Copies the configuration and descriptors. A NULL scope is session-wide.
   A native render/callback host user pointer scopes the rows to that host.
   The callback and user must stay valid until unregister returns OK.
   A busy unregister can be retried. Registration does not apply values.
   Startup readers decode saved text through the shared profile-text codec. */
RSF_RUNTIME_API int32_t rsf_overlay_saved_options_register(void* scope, const rsf_overlay_saved_setup*);
RSF_RUNTIME_API int32_t rsf_overlay_saved_options_unregister(void* scope);

#ifdef __cplusplus
}
#endif
