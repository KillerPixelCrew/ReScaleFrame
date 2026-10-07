/* SPDX-License-Identifier: GPL-3.0-only */
/**
 * @file
 * Compile frame-generation controller headers as C, without a runtime test.
 * Static assertions pin the ABI version and generated-frame field width. Keeping an
 * OBJECT target catches accidental C++ dependencies without requiring an entry point.
 */
#include <rescaleframe/fg_session.h>
#include <rescaleframe/fg_leases.h>
#include <rescaleframe/frame_sequencer.h>
#include <rescaleframe/render_links.h>
#include <rescaleframe/streamline_host.h>
_Static_assert(RSF_FG_ABI_VERSION == 1u, "Update the C contract fixture for an ABI change");
_Static_assert(sizeof(((rsf_fg_options*)0)->generated_frames) == 4, "C ABI generated count");
