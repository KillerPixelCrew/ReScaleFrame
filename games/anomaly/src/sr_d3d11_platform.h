/* SPDX-License-Identifier: MIT */
#pragma once
/* The pipeline-state snapshot sr_d3d11_platform.c lends the Rust SR owners (sr_d3d11/native.rs declares and calls
   both). Rust owns twin creation, the passes and the typed COM calls. */

#ifdef __cplusplus
#define RSF_SR_D3D11_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_SR_D3D11_NOEXCEPT
#endif

/* An owned snapshot of the immediate-context pipeline or null; the restore frees it. */
void* rsf_anomaly_sr_save_state(void* context) RSF_SR_D3D11_NOEXCEPT;
void rsf_anomaly_sr_restore_state(void* context, void* snapshot) RSF_SR_D3D11_NOEXCEPT;

#ifdef __cplusplus
}
#endif
#undef RSF_SR_D3D11_NOEXCEPT
