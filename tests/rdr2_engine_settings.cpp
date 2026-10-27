// SPDX-License-Identifier: GPL-3.0-only
#include "../games/rdr2/src/engine_settings_policy.h"
#include <array>
#include <cstdio>
int main()
{
    // Measured active capability list omits unsupported Ultra Quality4, while
    // the freshly constructed pending owner initially contains it. Indices move.
    const uint32_t active_modes[]{5,6,2,1,0,3}, initial_modes[]{5,6,4,2,1,0,3};
    const uint32_t active_indices[]{2,2,3,4,5}, initial_indices[]{3,3,4,5,6};
    for(uint32_t quality=0;quality<5;++quality)
        if(rsf_rdr2_dlss_choice(active_modes,6,quality)!=active_indices[quality] ||
            rsf_rdr2_dlss_choice(initial_modes,7,quality)!=initial_indices[quality])return 1;
    if(rsf_rdr2_dlss_choice(nullptr,6,1)!=UINT32_MAX || rsf_rdr2_dlss_choice(active_modes,8,1)!=UINT32_MAX ||
        rsf_rdr2_dlss_choice(active_modes,6,5)!=UINT32_MAX)return 1;
    std::array<unsigned char, 0x180> original{}; original.fill(0x5a);
    const uint16_t version = 37; std::memcpy(original.data() + 0x178, &version, 2);
    for (const auto vendor : {0x10deu, 0x1002u, 0x8086u, 0u}) {
        auto data = original, expected = original;
        const uint32_t dlss = vendor == 0x10de ? 3u : 0u, fsr = vendor == 0x10de ? 0u : 2u, markers = 1;
        const uint32_t quality = vendor == 0x10de ? 1u : 5u;
        std::memcpy(expected.data() + 0xe8, &dlss, 4);
        std::memcpy(expected.data() + 0xec, &quality, 4);
        std::memcpy(expected.data() + 0xf4, &fsr, 4);
        std::memcpy(expected.data() + 0x15c, &markers, 4);
        if (!rsf_rdr2_pin_engine_settings(data.data(), data.size(), vendor) || data != expected) return 1;
        for (uint32_t selection=0;selection<5;++selection) {
            const uint32_t dlss_modes[]{2,2,1,0,3}, dlss_indices[]{2,2,3,4,5}, fsr_modes[]{1,1,2,3,3};
            auto preset=original, wanted=original;
            const uint32_t index=vendor==0x10de?dlss_indices[selection]:0;
            const uint32_t mode=vendor==0x10de?dlss_modes[selection]:5;
            const uint32_t fsr_mode=vendor==0x10de?0:fsr_modes[selection];
            std::memcpy(wanted.data()+0xe8,&index,4);std::memcpy(wanted.data()+0xec,&mode,4);
            std::memcpy(wanted.data()+0xf4,&fsr_mode,4);std::memcpy(wanted.data()+0x15c,&markers,4);
            if (!rsf_rdr2_pin_engine_settings(preset.data(),preset.size(),vendor,selection) || preset!=wanted) return 1;
        }
        const uint32_t off = 0; std::memcpy(data.data() + (dlss ? 0xe8 : 0xf4), &off, 4);
        if (!rsf_rdr2_pin_engine_settings(data.data(), data.size(), vendor) || data != expected) return 1;
    }
    auto defaults = original; const uint16_t zero_version = 0; std::memcpy(defaults.data() + 0x178, &zero_version, 2);
    if (!rsf_rdr2_pin_engine_settings(defaults.data(), defaults.size(), 0x1002)) return 1;
    auto invalid = original; const uint32_t bad_version = 1; std::memcpy(invalid.data() + 0x178, &bad_version, 4);
    const auto unchanged = invalid;
    if (rsf_rdr2_pin_engine_settings(invalid.data(), invalid.size(), 0x10de) || invalid != unchanged ||
        rsf_rdr2_pin_engine_settings(original.data(), original.size() - 1, 0x10de) ||
        rsf_rdr2_pin_engine_settings(nullptr, original.size(), 0x10de) ||
        rsf_rdr2_pin_engine_settings(invalid.data(),invalid.size(),0x10de,5)) return 1;
    std::puts("Native carrier stays enabled for NVIDIA/AMD/Intel/unknown; unrelated fields and invalid objects stay unchanged.");
}
