/* SPDX-License-Identifier: MIT */
#pragma once
#include <dx12/ffx_api_dx12.h>
#include <ffx_upscale.h>

/* Expose the macro expressions to bindgen as evaluated constants. */
enum rsf_ffx_descriptor_type {
    RSF_FFX_CREATE_BACKEND_DX12 = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12,
    RSF_FFX_CREATE_UPSCALE = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE,
    RSF_FFX_DISPATCH_UPSCALE = FFX_API_DISPATCH_DESC_TYPE_UPSCALE,
    RSF_FFX_QUERY_UPSCALE_SIZE = FFX_API_QUERY_DESC_TYPE_UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE,
};
