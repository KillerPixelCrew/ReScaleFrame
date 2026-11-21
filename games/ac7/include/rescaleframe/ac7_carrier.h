/* SPDX-License-Identifier: MIT */
#ifndef RESCALEFRAME_AC7_CARRIER_H
#define RESCALEFRAME_AC7_CARRIER_H

/* Private interface between rsf-ac7 and the two combined carriers. */
#include <stddef.h>
#include <rescaleframe/game_api.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Process selection is a name check. GameSDK detection still checks the executable hash. */
const wchar_t* rsf_ac7_carrier_executable(void);
const wchar_t* rsf_ac7_carrier_game_dll(void);
const wchar_t* rsf_ac7_carrier_window_class(void);
/* Borrows sha256 for the same duration as the caller's GameSDK probe. */
int rsf_ac7_carrier_probe(rsf_game_probe* probe, const char* sha256);

/* rsf_patch_code, including its optional expected window and previous-byte output. */
typedef int32_t (*rsf_ac7_carrier_patch_fn)(uint32_t rva, const uint8_t* bytes, uint32_t count,
    const uint8_t* expected, uint32_t expected_count, uint8_t* previous);

#define RSF_AC7_CARRIER_ABI_VERSION 1u
typedef struct rsf_ac7_carrier_leaves {
    uint32_t struct_size;
    uint32_t abi_version;
    uint32_t (*read_number)(const char* name, uint32_t fallback);
    rsf_ac7_carrier_patch_fn patch;
    rsf_game_log_fn log;
    void (*install_jitter)(int32_t mode, uint32_t rva, int main_view_only);
    void (*set_depth_conformed)(int conformed);
    void (*register_translucency_scale)(uint32_t rva, uint32_t immediate_offset);
    int (*prepare_native_game)(void);
} rsf_ac7_carrier_leaves;

/* Called once by the worker after the existing decryption wait succeeds. */
void rsf_ac7_carrier_install_decrypted(const rsf_ac7_carrier_leaves* leaves);

/* Opens or closes the verified gate. stub_rel32 is null for the all-views instruction.
   previous must have room for six bytes. Returns the native patch result. */
int32_t rsf_ac7_carrier_patch_jitter(rsf_ac7_carrier_patch_fn patch, uint32_t rva, int enabled,
    const int32_t* stub_rel32, uint8_t previous[6]);

#ifdef __cplusplus
}
#endif
#endif
