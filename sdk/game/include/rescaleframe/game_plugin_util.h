/* SPDX-License-Identifier: MIT */
/* Small helpers every game plugin needs: the struct_size/abi_version check on host-supplied
 * structs, and case-insensitive ASCII comparison for executable name and hash detection.
 *
 * C++ only, header-only and self-contained like the rest of the game SDK: it includes game_api.h
 * and nothing else from this repository. */

#ifndef RSF_GAME_PLUGIN_UTIL_H
#define RSF_GAME_PLUGIN_UTIL_H

#include <stddef.h>
#include <rescaleframe/game_api.h>

#ifdef __cplusplus
namespace rsf_game {

/* A host struct must be at least as long as the plugin's version of it and carry the same ABI. */
template<class T> inline rsf_result validate(const T* value) noexcept
{
    if (!value || value->struct_size < sizeof(T)) return RSF_ERROR_INVALID_ARGUMENT;
    return value->abi_version == RSF_GAME_ABI_VERSION ? RSF_OK : RSF_ERROR_ABI_MISMATCH;
}

inline char ascii_lower(char value) noexcept
{
    return value >= 'A' && value <= 'Z' ? static_cast<char>(value + ('a' - 'A')) : value;
}

/* Null on either side is a mismatch. */
inline bool equal_ascii(const char* value, const char* expected) noexcept
{
    if (!value || !expected) return false;
    for (size_t i = 0;; ++i) {
        if (ascii_lower(value[i]) != ascii_lower(expected[i])) return false;
        if (expected[i] == '\0') return true;
    }
}

} /* namespace rsf_game */
#endif

#endif /* RSF_GAME_PLUGIN_UTIL_H */
