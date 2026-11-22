/* SPDX-License-Identifier: MIT */
/* Native leaf of the Rust Streamline bridge (lifecycle/src/bridge.rs): the
   interposer digest supplied at the build boundary. CMake and build.ps1 inject
   the corrected build receipt's digest; otherwise the pinned retail digest and
   its Authenticode check apply. Rust verifies and owns the file handle. */
#include <stdint.h>

void* rsf_slvk_open_verified(const uint16_t* path, const uint8_t* expected, uint32_t require_signature);

void* rsf_slvk_open_verified_release(const uint16_t* path)
{
    static const uint8_t expected[32] = {
#ifdef VULKANSTORY_CORRECTED_STREAMLINE_SHA256
        VULKANSTORY_CORRECTED_STREAMLINE_SHA256
#else
        0x8c, 0x87, 0xc9, 0x49, 0x94, 0x61, 0xda, 0x56,
        0x1e, 0xdd, 0x52, 0x9a, 0xa9, 0xbf, 0x78, 0x31,
        0xd6, 0x7d, 0x7b, 0x94, 0xeb, 0xb1, 0xc1, 0xa5,
        0xed, 0x54, 0xef, 0x49, 0x34, 0xe1, 0xea, 0x4c,
#endif
    };
#ifdef VULKANSTORY_CORRECTED_STREAMLINE_SHA256
    return rsf_slvk_open_verified(path, expected, 0);
#else
    return rsf_slvk_open_verified(path, expected, 1);
#endif
}
