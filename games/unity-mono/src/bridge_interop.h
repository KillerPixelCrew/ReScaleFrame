/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/submission_ring.h>

#ifdef __cplusplus
extern "C" {
#endif
/* Borrow the route's Runtime handle under its callback or retirement lease. */
const rsf_submission_ring* rsf_unity_d3d11_submission_ring(void);
const rsf_submission_ring* rsf_unity_vulkan_submission_ring(void);
#ifdef __cplusplus
}
#endif
