// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/gpu_policy.h>
#include <rescaleframe/sr_session.h>
#include <cstdio>
int main()
{
    if (rsf_gpu_default_sr_backend(0x10de, 0) != RSF_SR_DLSS ||
        rsf_gpu_default_sr_backend(0x8086, 0) != RSF_SR_XESS ||
        rsf_gpu_default_sr_backend(0x1002, 0) != RSF_SR_FSR3 ||
        rsf_gpu_default_sr_backend(0, 0) != RSF_SR_FSR3 ||
        rsf_gpu_default_sr_backend(0x1414, 1) != RSF_SR_NONE ||
        rsf_gpu_default_sr_backend(0x10de, 1) != RSF_SR_NONE) return 1;
    std::puts("PASS: NVIDIA DLSS, Intel XeSS, AMD/unknown FSR3, software disabled.");
    return 0;
}
