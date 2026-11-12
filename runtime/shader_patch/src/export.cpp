// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/shader_patch.h>
extern "C" int32_t rsf_patch_query(rsf_shader_patch_api*);
extern "C" int32_t rsf_get_shader_patch_api(rsf_shader_patch_api* api) { return rsf_patch_query(api); }
