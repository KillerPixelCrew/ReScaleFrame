// SPDX-License-Identifier: MIT
#include <rescaleframe/fsr_backend.h>
#include <rescaleframe/xess_backend.h>
#include <cstdio>

int main()
{
    // The Rust fixture covers both capability tables. This host still verifies
    // the published getters and the loader for the actual build configuration.
    const rsf_sr_provider* providers[]{rsf_fsr_sr_provider(), rsf_xess_sr_provider()};
    const rsf_fg_provider* legacy[]{rsf_fsr_fg_provider(), rsf_xess_fg_provider()};
    for (unsigned i = 0; i < 2; ++i) {
        const auto* sr = providers[i];
        const auto* fg = legacy[i];
        if (!sr || !fg || sr->struct_size != sizeof(*sr) || fg->struct_size != sizeof(*fg) ||
            !sr->probe || !sr->open || !sr->evaluate || !fg->create_swapchain || !fg->tag_frame) {
            std::fputs("D3D12 backend contract table missing or incomplete\n", stderr);
            return 1;
        }
        rsf_backend_probe_desc probe{};
        probe.struct_size = sizeof(probe);
        probe.abi_version = RSF_BACKEND_ABI_VERSION;
        probe.game_api = RSF_API_D3D11;
        rsf_backend_caps caps{};
        caps.struct_size = sizeof(caps);
        const auto result = sr->probe(&probe, &caps);
        if (result != RSF_BACKEND_OK && result != RSF_BACKEND_ERROR_NOT_COMPILED) return 1;
        const bool compiled = result == RSF_BACKEND_OK;
        rsf_sr_open_desc missing{};
        missing.struct_size = sizeof(missing);
        missing.abi_version = RSF_BACKEND_ABI_VERSION;
        missing.api = RSF_API_D3D12;
        missing.device = &missing;
        missing.output_width = missing.output_height = 128;
        missing.fsr_major = 2;
        missing.runtime_directory_utf8 = "C:\\__rsf_missing_runtime_fixture__";
        void* session = nullptr;
        const auto expected = compiled ? RSF_BACKEND_ERROR_LOAD_FAILED : RSF_BACKEND_ERROR_NOT_COMPILED;
        if (sr->open(&missing, &session) != expected || session ||
            (sr->get_version != nullptr) != compiled) {
            std::fputs("D3D12 backend did not distinguish a missing runtime from a missing build\n", stderr);
            return 1;
        }
    }
    return 0;
}
