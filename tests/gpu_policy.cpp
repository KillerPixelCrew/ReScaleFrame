// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/gpu_policy.h>
#include <cstdio>
int main()
{
    if (rsf_gpu_default_sr_backend(0x10de, 0) != 1 ||
        rsf_gpu_default_sr_backend(0x8086, 0) != 5 ||
        rsf_gpu_default_sr_backend(0x1002, 0) != 3 ||
        rsf_gpu_default_sr_backend(0, 0) != 3 ||
        rsf_gpu_default_sr_backend(0x1414, 1) != 0 ||
        rsf_gpu_default_sr_backend(0x10de, 1) != 0) return 1;
    std::puts("PASS: NVIDIA DLSS, Intel XeSS, AMD/unknown FSR3, software disabled.");
    return 0;
}
