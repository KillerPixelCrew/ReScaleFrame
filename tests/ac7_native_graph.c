/* SPDX-License-Identifier: MIT */
/* Launches the AC7 native graph fixture (tests/ac7_native_graph.rs, in the rsf-ac7 crate with the
   warp-fixture feature) with the replay helpers of ac7_view_fixture.rs, the runtime's SR entry,
   the render scope create call and the native renderer's call table. Exit code 77 skips when no
   WARP device exists. */
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "ac7_graph_replay.h"
#include "../games/ac7/src/ac7_sites.h"
#include <rescaleframe/native_sr.h>

bool rsf_ac7_graph_fixture_view(float* output, size_t size);
bool rsf_ac7_graph_fixture_velocity_patches(unsigned char* image, size_t size);
bool rsf_ac7_graph_fixture_hooks(const uint32_t* hooks, size_t count, uint32_t output_role);

/* Mirrored by `Calls` in tests/ac7_native_graph.rs. */
typedef struct rsf_ac7_native_graph_calls {
    void (*replay_camera)(unsigned char* module, size_t size, rsf_ac7_graph_screen_fn screen,
        rsf_ac7_graph_check_fn check);
    void (*replay_particles)(const rsf_ac7_graph_particle_replay* replay);
    void (*replay_truesky)(unsigned char* image, size_t size, void* owner,
        const struct rsf_ac7_truesky_patches* patches, void* handler,
        rsf_ac7_graph_sky_apply_fn apply, rsf_ac7_graph_sky_restore_fn restore,
        rsf_ac7_graph_check_fn check);
    bool (*fixture_view)(float* output, size_t size);
    bool (*velocity_patches)(unsigned char* image, size_t size);
    bool (*fixture_hooks)(const uint32_t* hooks, size_t count, uint32_t output_role);
    rsf_dlss_pipeline_result (*native_sr_evaluate)(void* context, const rsf_game_render_pass* pass);
    int (*scopes_create)(uint64_t session_id, uint32_t capacity, rsf_ac7_render_scope_fn callback,
        void* user, rsf_ac7_render_scopes** out);
    const rsf_ac7_native_calls* native;
} rsf_ac7_native_graph_calls;

int rsf_ac7_native_graph_fixture(const rsf_ac7_native_graph_calls* calls);

int main(void)
{
    const rsf_ac7_native_graph_calls calls = {
        rsf_ac7_graph_replay_camera,
        rsf_ac7_graph_replay_particles,
        rsf_ac7_graph_replay_truesky,
        rsf_ac7_graph_fixture_view,
        rsf_ac7_graph_fixture_velocity_patches,
        rsf_ac7_graph_fixture_hooks,
        rsf_native_sr_evaluate,
        rsf_ac7_render_scopes_create,
        &rsf_ac7_native_calls_table,
    };
    return rsf_ac7_native_graph_fixture(&calls);
}
