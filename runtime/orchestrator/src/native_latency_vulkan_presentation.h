// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg_vulkan.h>

#ifdef __cplusplus
extern "C" {
#endif
rsf_backend_result rsf_core_fgvk_presentation_attach(rsf_fgvk*);
rsf_backend_result rsf_core_fgvk_presentation_unbind();
rsf_fgvk* rsf_core_fgvk_presentation_borrow(uint32_t* closing);
void rsf_core_fgvk_presentation_return();
rsf_backend_result rsf_core_fgvk_presentation_shutdown_begin();
void rsf_core_fgvk_presentation_shutdown_end(uint32_t success);
#ifdef __cplusplus
}
#endif
