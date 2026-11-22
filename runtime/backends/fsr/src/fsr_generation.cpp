// SPDX-License-Identifier: MIT
#include "../../common/generation12_checks.h"
#include "../../common/frame_capture12.h"
#if defined(RSF_HAVE_FFX_FG)
extern "C" rsf_backend_result rsf_core_fsr12_create_native(const rsf_gen12_checks*,
    const rsf_core_capture12_platform*, const rsf_generation_setup*, void**, void**);
extern "C" uint32_t rsf_core_fsr12_probe_native(const rsf_gen12_checks*, const rsf_generation_setup*);

extern "C" const rsf_generation_provider* rsf_generation_fsr()
{
    static const auto provider = [] {
        auto value = *rsf_core_fsr12_provider();
        value.create = [](const rsf_generation_setup* setup, void** out, void** chain) {
            return rsf_core_fsr12_create_native(&rsf::generation12_checks, rsf::FrameCapture12::native_platform(), setup, out, chain);
        };
        return value;
    }();
    return &provider;
}

extern "C" uint32_t rsf_generation_fsr4_supported(void* device, const char* directory)
{
    if (!device || !directory || !*directory) return 0;
    rsf_generation_setup setup{};
    setup.runtime_directory_utf8 = directory;
    setup.chain.d3d12_device = device;
    setup.feature_major = 4;
    return rsf_core_fsr12_probe_native(&rsf::generation12_checks, &setup);
}
#else
extern "C" const rsf_generation_provider* rsf_generation_fsr() { return rsf::not_compiled_provider(); }
extern "C" uint32_t rsf_generation_fsr4_supported(void*, const char*) { return 0; }
#endif
