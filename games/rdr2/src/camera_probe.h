// SPDX-License-Identifier: MIT
#pragma once

extern "C" bool rsf_rdr2_install_camera_probe(const wchar_t* directory) noexcept;
extern "C" bool rsf_rdr2_install_camera_reference(void* self, const wchar_t* library) noexcept;

#ifdef RSF_RDR2_CAPTURE_TEST
void rsf_rdr2_request_camera_capture() noexcept;
#else
extern "C" void rsf_rdr2_camera_request() noexcept;
inline void rsf_rdr2_request_camera_capture() noexcept { rsf_rdr2_camera_request(); }
#endif
