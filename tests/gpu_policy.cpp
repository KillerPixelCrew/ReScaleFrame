// SPDX-License-Identifier: MIT
#include <rescaleframe/gpu_policy.h>
#include <rescaleframe/sr_session.h>
#include <rescaleframe/frame_generation.h>
#include <cstdio>
int main()
{
    if (rsf_gpu_default_sr_backend(0x10de, 0) != RSF_SR_DLSS ||
        rsf_gpu_default_sr_backend(0x8086, 0) != RSF_SR_XESS ||
        rsf_gpu_default_sr_backend(0x1002, 0) != RSF_SR_FSR3 ||
        rsf_gpu_default_sr_backend(0, 0) != RSF_SR_FSR3 ||
        rsf_gpu_default_sr_backend(0x1414, 1) != RSF_SR_NONE ||
        rsf_gpu_default_sr_backend(0x10de, 1) != RSF_SR_NONE) return 1;
    constexpr uint32_t all = (1u << RSF_SR_DLSS) | (1u << RSF_SR_FSR3) | (1u << RSF_SR_FSR4) | (1u << RSF_SR_XESS);
    if (rsf_gpu_select_sr_backend(0x8086, 0, all) != RSF_SR_XESS ||
        rsf_gpu_select_sr_backend(0x10de, 0, all) != RSF_SR_DLSS ||
        rsf_gpu_select_sr_backend(0x1002, 0, all) != RSF_SR_FSR3 ||
        rsf_gpu_select_sr_backend(0x8086, 0, 1u << RSF_SR_FSR3) != RSF_SR_FSR3 ||
        rsf_gpu_select_sr_backend(0x1002, 0, 1u << RSF_SR_XESS) != RSF_SR_XESS ||
        rsf_gpu_select_sr_backend(0x10de, 0, 1u << RSF_SR_FSR4) != RSF_SR_NONE ||
        rsf_gpu_select_sr_backend(0x8086, 1, all) != RSF_SR_NONE) return 1;
    // FG: the header preference where served, then FSR3, as the generic D3D12 hosts resolve it.
    for (const uint32_t vendor : {0x10deu, 0x8086u, 0x1002u, 0u, 0x1414u, 0xffffu})
        for (const uint32_t software : {0u, 1u}) {
            const uint32_t preferred = rsf_fg_default_backend(vendor, software);
            const uint32_t expected = preferred ? preferred : RSF_FG_BACKEND_FSR3;
            if (rsf_gpu_select_fg_backend(vendor, software, RSF_FG_BACKEND_ALL) != expected) return 1;
        }
    if (rsf_gpu_select_fg_backend(0x10de, 0, 1u << RSF_FG_BACKEND_FSR3) != RSF_FG_BACKEND_FSR3 ||
        rsf_gpu_select_fg_backend(0x8086, 0, 1u << RSF_FG_BACKEND_DLSS) != 0 ||
        rsf_gpu_select_fg_backend(0x1002, 0, 1u << RSF_FG_BACKEND_FSR4) != 0 ||
        rsf_gpu_select_fg_backend(0x10de, 0, 0) != 0 || rsf_gpu_select_fg_backend(0x10de, 0, 1u) != 0) return 1;
    std::puts("PASS: NVIDIA DLSS, Intel XeSS, AMD/unknown FSR3, software disabled, choice fallback, FG Auto.");
    return 0;
}
