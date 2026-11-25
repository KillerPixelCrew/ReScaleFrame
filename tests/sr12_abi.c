// SPDX-License-Identifier: MIT
// CPU-only refusals of the generic D3D12 SR entries. No device is created or dereferenced: every
// setup below is refused before the owner touches its device.
#include <rescaleframe/native_sr_d3d12.h>
#include <stddef.h>
#include <stdio.h>

#define CHECK(condition) do { if (!(condition)) { printf("SR12 ABI failed at line %d\n", __LINE__); return 1; } } while (0)

_Static_assert(sizeof(rsf_sr12_setup) == 216 && offsetof(rsf_sr12_setup, engine_input_extents) == 196,
    "SR12 setup layout");

static int refused(const rsf_sr12_setup* setup, void* placeholder, rsf_backend_result expected)
{
    rsf_sr12* created = placeholder;
    return rsf_sr12_create(setup, &created) == expected && !created;
}

int main(void)
{
    int placeholder = 0;
    rsf_sr12_setup setup = {0};
    setup.struct_size = sizeof(setup);
    setup.abi_version = 2;
    setup.device = &placeholder;
    setup.output_width = 1920;
    setup.output_height = 1080;
    rsf_sr12* owner = (rsf_sr12*)&placeholder;

    CHECK(rsf_sr12_create(&setup, NULL) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_sr12_create(NULL, &owner) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !owner);
    rsf_sr12_setup changed = setup; changed.struct_size = offsetof(rsf_sr12_setup, engine_input_extents) - 1;
    CHECK(refused(&changed, &placeholder, RSF_BACKEND_ERROR_INVALID_ARGUMENT));
    changed = setup; changed.abi_version = 3;
    CHECK(refused(&changed, &placeholder, RSF_BACKEND_ERROR_ABI_MISMATCH));
    changed = setup; changed.struct_size = sizeof(setup) - sizeof(void*);
    CHECK(refused(&changed, &placeholder, RSF_BACKEND_ERROR_INVALID_ARGUMENT));
    /* Spatial (FSR1) is a served backend and is admitted past this check, so it is not used here:
       admission would reach the placeholder device. Auto and anything past it are refused. */
    changed = setup; changed.backend = RSF_SR_AUTO;
    CHECK(refused(&changed, &placeholder, RSF_BACKEND_ERROR_INVALID_ARGUMENT));
    changed = setup; changed.backend = RSF_SR_AUTO + 1;
    CHECK(refused(&changed, &placeholder, RSF_BACKEND_ERROR_INVALID_ARGUMENT));
    changed = setup; changed.quality = 6;
    CHECK(refused(&changed, &placeholder, RSF_BACKEND_ERROR_INVALID_ARGUMENT));
    changed = setup; changed.device = NULL;
    CHECK(refused(&changed, &placeholder, RSF_BACKEND_ERROR_INVALID_ARGUMENT));
    changed = setup; changed.engine_input_extents = 1; changed.render_width = 1921; changed.render_height = 1080;
    CHECK(refused(&changed, &placeholder, RSF_BACKEND_ERROR_INVALID_ARGUMENT));

    uint32_t width = 0, height = 0;
    CHECK(rsf_sr12_plan(NULL, &width, &height) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    rsf_game_render_pass pass = {0};
    pass.struct_size = sizeof(pass);
    CHECK(rsf_sr12_evaluate(NULL, &placeholder, &pass, 0) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_sr12_capture_scene(NULL, &placeholder, &pass, 0) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    rsf_sharpening_options options = rsf_sharpening_defaults();
    CHECK(rsf_sr12_set_sharpening(NULL, &options) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_sr12_present(NULL, &placeholder, 0, 0, 0) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_sr12_capture(NULL, "C:\\", 1, 1) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_sr12_capture_flush(NULL, 0) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    rsf_sr12_destroy(NULL);
    return 0;
}
