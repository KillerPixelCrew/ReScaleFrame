/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_D3D11_PRESENT_BRIDGE_H
#define RSF_D3D11_PRESENT_BRIDGE_H
#include <rescaleframe/streamline_host.h>
#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/d3d11_chain_dispatch.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_d3d11_present_setup {
    uint32_t struct_size;
    const char* runtime_directory_utf8;
    uint32_t development_runtime;
    rsf_backend_log_fn log;
    void* user;
    int (*accept_window)(void* user, void* hwnd);
    rsf_observer_present_fn before_present;
    rsf_observer_present_event_fn present_event;
    rsf_observer_present_event_fn latency_event;
    void (*retire)(void* user, void* provider_session);
    /* Graphics owner, after upload to the physical backbuffer, before submission/Present.
       The list is open. All pointers are borrowed for this callback. */
    void (*prepare)(void* user, void* chain11, void* context11, void* list12, void* backbuffer12,
                    rsf_streamline_host* host, void* provider_session, uint32_t sync_interval);
    uint32_t debug_timing;
    /* Cold-start generation owner, independent of SR. Zero retains the DLSS default.
       Also accepts native D3D12 queues; no D3D11 facade or upload is used on that path. */
    uint32_t backend, max_generated_frames;
    /* The plugin's depth conventions. units_to_meters must be > 0; install refuses otherwise,
       since the bridge cannot tell an engine's units from its graphics API. */
    uint32_t depth_inverted, depth_infinite;
    float units_to_meters;
    /* Native D3D12 keeps engine buffers stable while swapping the physical presentation
       provider at a drained Present boundary. Off retains the facade and plain presentation. */
    uint32_t runtime_switching, ui_mode;
    const char* fsr3_directory_utf8;
    const char* fsr4_directory_utf8;
    const char* xess_directory_utf8;
    const char* streamline_directory_utf8;
    /* Engine identity for Streamline (sl::EngineType value, version, project id), supplied by the
       plugin or engine.json. Without a version and project id there is no Streamline host, so
       DLSS-G is unavailable; other generation backends are unaffected. */
    uint32_t engine_type;
    const char* engine_version_utf8;
    const char* project_id_utf8;
    /* Zero retains all providers. Nonzero constrains cold selection and every switch. */
    uint32_t allowed_backend_choices;
} rsf_d3d11_present_setup;
/* Cold startup only, before the game's main swapchain. Unsupported creation forwards unchanged.
   No game-specific sites or policies live in the bridge. Hooks remain for process lifetime. */
int rsf_d3d11_present_install(const rsf_d3d11_present_setup* setup);
/* Same factory hooks, forwarding unchanged outside an explicit thread/window
   transaction. Tokens are process-unique; all graphics inputs remain borrowed. */
int rsf_d3d11_present_install_scoped(const rsf_d3d11_present_setup* setup);
int32_t rsf_d3d11_present_creation_begin(void* window, uint64_t* token);
int32_t rsf_d3d11_present_creation_end(void* window, uint64_t token, void* device, void* context, void* outer_chain);
/* Engine integrations own the creation call and need no DXGI detours. Prepare
   once before device/swapchain creation, then call create_for_hwnd at that site.
   All arguments are borrowed DXGI objects/descriptors; out owns one COM reference. */
int rsf_d3d11_present_prepare(const rsf_d3d11_present_setup* setup);
int32_t rsf_d3d11_present_create_for_hwnd(void* factory, void* queue, void* hwnd,
    const void* description, void** out);
/* Internal cold mechanism, after the outer D3D11 creation wrappers return.
   Inputs borrowed; out owns one facade reference. Native chain is never consumed. */
int32_t rsf_d3d11_present_wrap_native_chain(void* device, void* native_chain, void** out);
rsf_streamline_host* rsf_d3d11_present_host(void);
/* D3D11 execution thread: shared device plus the dedicated SR/upload queue, borrowed through
   process lifetime. native_queue is null; this proxy queue differs from the FG present queue. */
int rsf_d3d11_present_graphics(rsf_streamline_graphics* graphics);
/* Present owner only. Borrowed creation queue for the current physical provider, distinct
   from the D3D11 SR/upload queue. Valid until the next drained provider transition. */
void* rsf_d3d11_present_queue(void);
/* The list a provider-queue consumer should record into for a list created on the shared
   device: the native list behind a Streamline proxy unless the provider queue is Streamline's.
   Null when it cannot be unwrapped. Borrowed for the list's lifetime. */
void* rsf_d3d11_present_command_list(void* list);
/* Retire callback only. Joins the session's vendor retirement fence on the provider queue, then
   executes list (closed, created on the shared device, may be null) there, then signals a private
   fence. On success fence/value cover vendor reads, the list and all earlier provider-queue work;
   the fence is borrowed (AddRef to keep it). On failure nothing was signalled and both are zero. */
int32_t rsf_d3d11_present_retire(void* provider_session, void* list, void** fence, uint64_t* value);
int rsf_d3d11_present_is_owner(void* swapchain);
/* Borrowed live COM receiver. Exact inner facade only, including a refused owner;
   no alias QI or unowned dereference. Returns 1 and writes the native result when
   handled, otherwise 0 with result unchanged. Does not reenter the virtual slot. */
int rsf_d3d11_present_dispatch_owned(void* receiver, uint32_t sync_interval, uint32_t flags, int32_t* result);
/* Host validates header/method. Writes handled before calling the typed body;
   exact inner receiver only, including Release during refusal or cancellation. */
void rsf_d3d11_chain_dispatch_owned(rsf_d3d11_chain_dispatch* request);
int rsf_d3d11_present_has_owner(void);
const rsf_generation_provider* rsf_d3d11_present_provider(void);
void* rsf_d3d11_present_session(void);
uint32_t rsf_d3d11_present_backend(void);
/* Choice mask, with FSR4 FG enabled only when the SDK exposes major 4 for the engine device. */
uint32_t rsf_d3d11_present_backend_choices(void);
int32_t rsf_d3d11_present_request(uint32_t backend);
int32_t rsf_d3d11_present_switch_result(void);
uint64_t rsf_d3d11_present_generation(void);
/* Main-thread calls skip a provider during retirement, rather than blocking window messages. */
int32_t rsf_d3d11_present_begin(uint64_t id, uint64_t* generation);
/* Runtime-only mechanism: the core supplies the chosen owner. This function never chooses another pacer. */
int32_t rsf_d3d11_present_pace(uint64_t id, rsf_pacing_owner owner, uint64_t* generation);
int32_t rsf_d3d11_present_marker(uint64_t generation, uint64_t id, rsf_latency_marker marker, uint32_t controller);
int32_t rsf_d3d11_present_acquire(uint64_t id);
int32_t rsf_d3d11_present_input(uint64_t id, uint32_t kinds, uint32_t message);
int32_t rsf_d3d11_present_abort(uint64_t id);
#ifdef __cplusplus
}
#endif
#endif
