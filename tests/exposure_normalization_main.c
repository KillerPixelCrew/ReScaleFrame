/* SPDX-License-Identifier: MIT */
#include <rescaleframe/native_sr_d3d12.h>

typedef struct {
    rsf_backend_result (*create)(const rsf_sr12_setup*, rsf_sr12**);
    rsf_backend_result (*capture)(rsf_sr12*, const char*, uint64_t, uint32_t);
    rsf_backend_result (*evaluate)(rsf_sr12*, void*, const rsf_game_render_pass*, uint32_t);
    rsf_backend_result (*flush)(rsf_sr12*, uint32_t);
    void (*destroy)(rsf_sr12*);
} exposure_fixture_services;

extern int32_t rsf_exposure_normalization_fixture(const exposure_fixture_services*);

int main(void)
{
    const exposure_fixture_services services = {
        rsf_sr12_create, rsf_sr12_capture, rsf_sr12_evaluate, rsf_sr12_capture_flush, rsf_sr12_destroy,
    };
    return rsf_exposure_normalization_fixture(&services);
}
