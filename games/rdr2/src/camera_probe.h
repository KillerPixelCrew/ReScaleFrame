// SPDX-License-Identifier: GPL-3.0-only
#pragma once

// Opt-in research only. The directory must exist before the first capture request.
bool rsf_rdr2_install_camera_probe(const wchar_t* directory) noexcept;
bool rsf_rdr2_install_camera_reference(void* self, const wchar_t* library) noexcept;
void rsf_rdr2_request_camera_capture() noexcept;
