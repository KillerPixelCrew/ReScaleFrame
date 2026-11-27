// SPDX-License-Identifier: MIT
// Test helper: which exact Anomaly build a profile-dependent fixture runs against. CTest registers those fixtures once
// per build; the argument "standard" (default) or "avx" selects it. The Rust fixture archive owns the selection.
#pragma once
#include <stddef.h>

#ifdef __cplusplus
#define RSF_BUILD_SELECT_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_BUILD_SELECT_NOEXCEPT
#endif

// Index of the selected build in native_profile.rs order (Standard 0, AVX 1).
size_t rsf_test_anomaly_select_build(int argc, const char* const* argv) RSF_BUILD_SELECT_NOEXCEPT;
// Prints the fixture banner for that build.
void rsf_test_anomaly_announce_build(const char* label, const char* executable) RSF_BUILD_SELECT_NOEXCEPT;

#ifdef __cplusplus
}
#endif
#undef RSF_BUILD_SELECT_NOEXCEPT
