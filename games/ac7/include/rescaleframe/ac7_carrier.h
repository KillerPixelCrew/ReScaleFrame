/* SPDX-License-Identifier: MIT */
#ifndef RESCALEFRAME_AC7_CARRIER_H
#define RESCALEFRAME_AC7_CARRIER_H

/* Private interface between rsf-ac7 and the AC7 carrier. */
#include <stddef.h>
#include <rescaleframe/game_api.h>

#ifdef __cplusplus
extern "C" {
#endif

const wchar_t* rsf_ac7_carrier_game_dll(void);
const wchar_t* rsf_ac7_carrier_window_class(void);
/* Borrows sha256 for the same duration as the caller's GameSDK probe. */
int rsf_ac7_carrier_probe(rsf_game_probe* probe, const char* sha256);

/* rsf_patch_code, including its optional expected window and previous-byte output. */
typedef int32_t (*rsf_ac7_carrier_patch_fn)(uint32_t rva, const uint8_t* bytes, uint32_t count,
    const uint8_t* expected, uint32_t expected_count, uint8_t* previous);

#define RSF_AC7_CARRIER_ABI_VERSION 4u
typedef struct rsf_ac7_carrier_leaves {
    uint32_t struct_size;
    uint32_t abi_version;
    uint32_t (*read_number)(const char* name, uint32_t fallback);
    rsf_ac7_carrier_patch_fn patch;
    rsf_game_log_fn log;
    void (*install_jitter)(int32_t mode, uint32_t rva);
    int (*prepare_native_game)(void);
    int (*install_motion_capture)(void);
} rsf_ac7_carrier_leaves;

/* Runs patches, native preparation and optional capture install, then publishes worker readiness.
   Called once by the worker after the decryption wait succeeds. */
void rsf_ac7_carrier_install_decrypted(const rsf_ac7_carrier_leaves* leaves);
/* Measure-only decryption gate. The caller supplies the native measure and timeout results. */
int32_t rsf_ac7_carrier_wait_decrypted(int32_t (*measure)(double*), int32_t still_encrypted);
/* Used by the Present callback to gate the native renderer's start. */
unsigned long rsf_ac7_carrier_startup_ready(void);

/* Numeric startup choices defined by the adapter. A returned key and label live for the process.
   Returns null at the end of the list without writing the output pointers. */
#define RSF_AC7_CARRIER_START_OPTION_COUNT 5u
const char* rsf_ac7_carrier_start_option(uint32_t index, const char** label,
    uint32_t* fallback, uint32_t* maximum);

/* Opens or closes the verified gate. previous must have room for six bytes. Returns the native
   patch result. */
int32_t rsf_ac7_carrier_patch_jitter(rsf_ac7_carrier_patch_fn patch, uint32_t rva, int enabled,
    uint8_t previous[6]);

#ifdef __cplusplus
}
#endif
#endif
