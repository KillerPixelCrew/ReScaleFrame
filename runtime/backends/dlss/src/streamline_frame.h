// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/streamline_host.h>
#if RSF_HAVE_STREAMLINE
#include <sl.h>
// Internal to this backend module, never part of the public DLL or game ABI.
// Set common constants at most once per live source/viewport, taking marker_guard internally.
// Returns false for a missing/retired token, an already written viewport, or an SDK failure.
bool rsf_streamline_common_set(rsf_streamline_host*, uint64_t, uint32_t, const sl::Constants&);
// Query the same flag while the caller already holds marker_guard (FG prepare's lock).
bool rsf_streamline_common_exists(rsf_streamline_host*, uint64_t, uint32_t);
#endif
