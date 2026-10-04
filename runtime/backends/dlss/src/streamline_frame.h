// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/streamline_host.h>
#if RSF_HAVE_STREAMLINE
#include <sl.h>
// Internal to this backend module, never part of the public DLL or game ABI.
bool rsf_streamline_common_set(rsf_streamline_host*, uint64_t, uint32_t, const sl::Constants&);
bool rsf_streamline_common_exists(rsf_streamline_host*, uint64_t, uint32_t);
#endif
