// SPDX-License-Identifier: MIT
// DLSS preset admission and ABI assertions for the linked Rust OpenGL pipeline.
#include <rescaleframe/gl_pipeline.h>
#include <rescaleframe/dlss.h>
#include <rescaleframe/sr_session.h>
#include <rescaleframe/native_sr_d3d12.h>

// Values gl_pipeline.rs names directly.
_Static_assert(RSF_SR_NONE == 0 && RSF_SR_DLSS == 1 && RSF_SR_FSR2 == 2 && RSF_SR_FSR3 == 3 && RSF_SR_FSR4 == 4 &&
               RSF_SR_XESS == 5 && RSF_SR_SPATIAL == 6 && RSF_SR_AUTO == 7, "OpenGL SR backend IDs");
_Static_assert(RSF_DLSS_ENGINE_CUSTOM == 0, "OpenGL engine ID");
int rsf_gl_contract_dlss_preset_valid(uint32_t preset) { return rsf_dlss_preset_valid(preset); }
