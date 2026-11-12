/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_D3D11_FG_API_H
#define RSF_D3D11_FG_API_H

#include <rescaleframe/runtime.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/native_fg.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_D3D11_FG_API_VERSION 1u
#define RSF_D3D11_FG_API_ENTRY_POINT "rsf_get_d3d11_fg_api"

/* DLL access to the Runtime's one D3D11 frame-generation tagging path (native_fg.h), the
   path a statically linked carrier uses. It is not a second owner: the same CPU frame records, scene
   and window identity, shared transfer slots, presentation-bridge callbacks and retirement.
   Presentation itself is rsf_get_d3d11_present_api; this table feeds it.

   An adapter drives it with the game ABI's own vocabulary (game_renderer.h): CPU events for
   one real source frame, and FRAME, SUBMISSION, FINAL_SCENE, TEXTURE_BINDING and WINDOW
   render passes around that frame's work and its Present. Engine sites, screen policy and
   the HUD-less boundary stay in the adapter; nothing here selects game behaviour.

   Execution:
   - set_log/options/status/reflex_ordering: any thread. options takes effect at the next
     prepare; reflex_ordering must be set before the first frame.
   - cpu: the game thread's real CPU boundaries, in ABI order. It records the frame
     (rsf_native_cpu_event) and only then runs the latency/pacing work (rsf_native_fg_cpu):
     FRAME_BEGIN reserves the frame, PACING is the one sleep, INPUT_SAMPLE marks simulation
     start, SIMULATION_END marks its end. Returns the record's acceptance. Do not add another
     pacing owner for the same frame.
   - pass, bind_source, capture, inputs_ready, dump_inputs: the graphics thread that owns the
     D3D11 immediate context, which must also be the thread that calls the bridge's Present
     (scene and window identity are per-thread). pass dispatches one render pass exactly as
     the statically linked path does: FRAME and SUBMISSION to latency ownership, FINAL_SCENE to scene identity and,
     at its end, the HUD-less copy of scene_surface, TEXTURE_BINDING to the sampled-surface
     check, WINDOW to window scope (its end abandons an unpresented frame). Returns the
     identity owner's acceptance for FINAL_SCENE, TEXTURE_BINDING and WINDOW, else 1.
   - bind_source: rsf_native_fg_bind_source (native_fg.h). Lets Runtime SR passes that the
     adapter built without frame identity feed this source's depth and motion.
   - capture: copies this source's normalized depth (R32_FLOAT) and dense motion (R16G16
     or R32G32 float, render pixels, previous minus current) with the identity's camera.
     A no-op when the source already holds inputs.
   - prepare/retire/present_event/latency_event: place these in rsf_d3d11_present_setup
     (prepare, retire, present_event, latency_event). Each may be wrapped by the adapter's
     own callback that calls it with the same arguments.
   A populated table proves ABI availability only, not provider readiness. */
typedef struct rsf_d3d11_fg_api {
    uint32_t struct_size;
    uint32_t abi_version;
    void (*set_log)(rsf_backend_log_fn log, void* user);
    void (*options)(const rsf_native_fg_options* options);
    int (*status)(rsf_native_fg_status* status);
    void (*reflex_ordering)(uint32_t threaded);
    int (*cpu)(const rsf_game_cpu_event* event);
    int (*pass)(void* context11, const rsf_game_render_pass* pass, uint32_t begin);
    void (*bind_source)(const rsf_game_render_pass* identity);
    void (*capture)(void* context11, const rsf_game_render_pass* identity, const rsf_dlss_frame* frame);
    int (*inputs_ready)(uint64_t source_frame_id);
    int32_t (*dump_inputs)(void* context11, uint64_t source_frame_id, const char* directory_utf8,
                           const char* prefix_utf8);
    void (*prepare)(void* user, void* chain11, void* context11, void* list12, void* backbuffer12,
                    rsf_streamline_host* host, void* provider, uint32_t sync);
    void (*retire)(void* user, void* provider);
    void (*present_event)(void* user, const rsf_observer_present_event* event);
    void (*latency_event)(void* user, const rsf_observer_present_event* event);
} rsf_d3d11_fg_api;

/* Same contract as rsf_get_d3d11_present_api: initialize struct_size to the writable
   capacity and abi_version to RSF_D3D11_FG_API_VERSION. Null or capacity below
   sizeof(rsf_d3d11_fg_api): INVALID_ARGUMENT, without reading abi_version. Otherwise a
   version mismatch: ABI_MISMATCH. Failure writes nothing. On OK the table is filled and
   struct_size is the runtime's sizeof(table); bytes beyond it are untouched. Querying
   installs, loads and starts nothing. */
RSF_RUNTIME_API rsf_result rsf_get_d3d11_fg_api(rsf_d3d11_fg_api* api);
typedef rsf_result (*rsf_get_d3d11_fg_api_fn)(rsf_d3d11_fg_api* api);

#ifdef __cplusplus
}
#endif
#endif
