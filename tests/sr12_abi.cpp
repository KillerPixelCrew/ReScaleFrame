// SPDX-License-Identifier: MIT
// CPU-only refusals of the generic D3D12 SR entries. No device is created or dereferenced: every
// setup below is refused before the owner touches its device.
#include <rescaleframe/native_sr_d3d12.h>
#include <cstddef>
#include <cstdio>

#define CHECK(condition) do { if (!(condition)) { std::printf("SR12 ABI failed at line %d\n", __LINE__); return 1; } } while (false)

static_assert(sizeof(rsf_sr12_setup) == 216 && offsetof(rsf_sr12_setup, engine_input_extents) == 196);

int main()
{
    int placeholder = 0;
    rsf_sr12_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = 2;
    setup.device = &placeholder;
    setup.output_width = 1920;
    setup.output_height = 1080;
    rsf_sr12* owner = reinterpret_cast<rsf_sr12*>(&placeholder);

    CHECK(rsf_sr12_create(&setup, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_sr12_create(nullptr, &owner) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !owner);
    auto refused = [&](rsf_sr12_setup candidate, rsf_backend_result expected) {
        rsf_sr12* created = reinterpret_cast<rsf_sr12*>(&placeholder);
        return rsf_sr12_create(&candidate, &created) == expected && !created;
    };
    auto changed = setup; changed.struct_size = offsetof(rsf_sr12_setup, engine_input_extents) - 1;
    CHECK(refused(changed, RSF_BACKEND_ERROR_INVALID_ARGUMENT));
    changed = setup; changed.abi_version = 3;
    CHECK(refused(changed, RSF_BACKEND_ERROR_ABI_MISMATCH));
    changed = setup; changed.struct_size = sizeof(setup) - sizeof(void*);
    CHECK(refused(changed, RSF_BACKEND_ERROR_INVALID_ARGUMENT));
    changed = setup; changed.backend = RSF_SR_XESS + 1;
    CHECK(refused(changed, RSF_BACKEND_ERROR_INVALID_ARGUMENT));
    changed = setup; changed.quality = 6;
    CHECK(refused(changed, RSF_BACKEND_ERROR_INVALID_ARGUMENT));
    changed = setup; changed.device = nullptr;
    CHECK(refused(changed, RSF_BACKEND_ERROR_INVALID_ARGUMENT));
    changed = setup; changed.engine_input_extents = 1; changed.render_width = 1921; changed.render_height = 1080;
    CHECK(refused(changed, RSF_BACKEND_ERROR_INVALID_ARGUMENT));

    uint32_t width = 0, height = 0;
    CHECK(rsf_sr12_plan(nullptr, &width, &height) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    rsf_game_render_pass pass{};
    pass.struct_size = sizeof(pass);
    CHECK(rsf_sr12_evaluate(nullptr, &placeholder, &pass, 0) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_sr12_capture_scene(nullptr, &placeholder, &pass, 0) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    rsf_sharpening_options options = rsf_sharpening_defaults();
    CHECK(rsf_sr12_set_sharpening(nullptr, &options) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_sr12_present(nullptr, &placeholder, 0, 0, 0) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_sr12_capture(nullptr, "C:\\", 1, 1) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    CHECK(rsf_sr12_capture_flush(nullptr, 0) == RSF_BACKEND_ERROR_INVALID_ARGUMENT);
    rsf_sr12_destroy(nullptr);
    return 0;
}
