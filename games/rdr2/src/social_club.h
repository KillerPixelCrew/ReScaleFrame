// SPDX-License-Identifier: GPL-3.0-only
#pragma once

// Called only after the ASI validates the supported, decrypted RDR2 image.
bool rsf_rdr2_install_social_club_fix(void (*log)(void*, const char*), bool trace) noexcept;
