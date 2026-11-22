/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>
#include <rescaleframe/d3d11_present_bridge.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Private presentation bridge contract. d3d11_present.rs owns owner admission and
   publication, provider switching, frame and slot sequencing, accepted Present,
   fallback, resize and retirement policy, status and callback ordering, and latency
   routing. Leaves perform native DXGI, D3D12, D3D11, Win32 or SDK steps for the
   Rust IDXGISwapChain4 facade, whose native state owns the SDK and graphics
   pointers. Leaves do not let exceptions reach Rust. Native calls can reenter the facade. The Rust mirror in
   d3d11_present_abi.rs is hand-written; d3d11_present_bridge.cpp asserts the layouts. */

typedef struct rsf_d3d11p_chain rsf_d3d11p_chain;
typedef struct rsf_d3d11p_physical_lease rsf_d3d11p_physical_lease;

/* The application's creation request, normalized from DXGI_SWAP_CHAIN_DESC. */
typedef struct rsf_d3d11p_request {
    void* window;
    uint32_t width, height, format, sample_count;
    uint32_t buffer_count, windowed, flags, swap_effect;
    uint32_t refresh_numerator, refresh_denominator;
} rsf_d3d11p_request;

/* Current facade facts, read on demand from the native object. */
typedef struct rsf_d3d11p_facts {
    void* window;
    uint32_t width, height, format, buffer_count;
    uint32_t flags, windowed, native12, tearing;
    rsf_streamline_graphics graphics;
    void* direct_queue; /* engine queue on the native D3D12 path */
    void* device11;
    uint32_t buffer_usage;
} rsf_d3d11p_facts;

/* The plain physical chain's creation queue and DXGI overrides.
   Zero buffer_count or buffer_usage preserves the application's value. */
typedef struct rsf_d3d11p_plain_plan {
    void* queue;
    uint32_t swap_effect, flags, buffer_count, buffer_usage;
} rsf_d3d11p_plain_plan;

typedef struct rsf_d3d11p_adapter {
    uint32_t vendor, device, default_backend, reserved;
} rsf_d3d11p_adapter;

#define RSF_D3D11P_FAMILY_DLSS 1u
#define RSF_D3D11P_FAMILY_FIDELITYFX 2u
#define RSF_D3D11P_FAMILY_XESS 3u

typedef struct rsf_shared_surface_platform rsf_shared_surface_platform;
const rsf_shared_surface_platform* rsf_shared_surface_native_platform(void);

typedef struct rsf_d3d11p_platform {
    /* Process services. */
    const rsf_generation_provider* (*provider_table)(uint32_t family);
    void (*observer_present)(rsf_observer_present_event* event);
    uint32_t (*window_valid)(void* window);
    uint32_t (*client_size)(void* window, uint32_t* width, uint32_t* height);
    /* Compare the returned address only. The caller holds the live receiver. */
    void* (*canonical_identity)(void* receiver);
    void (*add_ref)(void* object);
    void (*release)(void* object);
    /* Streamline host. */
    int32_t (*host_sleep)(void* host, uint64_t id);
    int32_t (*host_marker)(void* host, uint64_t id, uint32_t marker, uint32_t controller);
    int32_t (*host_acquire)(void* host, uint64_t id);
    int32_t (*host_input)(void* host, uint64_t id, uint32_t kinds, uint32_t message);
    int32_t (*host_abort)(void* host, uint64_t id);
    int32_t (*host_reflex_off)(void* host);
    int32_t (*host_generation_load)(void* host, uint32_t enabled);
    int32_t (*host_present_common)(void* host, void* chain, uint32_t sync, uint32_t flags, uint32_t begin);

    /* Facade object. bridge_new returns one owned reference, or null. */
    void* (*bridge_new)(rsf_d3d11p_chain* chain);
    void (*bridge_release_owned)(void* bridge);
    rsf_d3d11p_chain* (*bridge_chain)(void* bridge);
    void (*facts)(void* bridge, rsf_d3d11p_facts* facts);

    /* Initialization, in call order. HRESULT unless noted. */
    uint32_t (*init_source)(void* bridge, void* factory, void* source, const void* desc); /* native12 */
    int32_t (*owning_adapter)(void* bridge, void* source, rsf_d3d11p_adapter* adapter);
    int32_t (*native_device)(void* bridge);
    /* Returns 1 when the owning adapter resolved and adoption was attempted (result set). */
    uint32_t (*adopt_host)(void* bridge, uint32_t identity, int32_t* result);
    int32_t (*d3d11_device)(void* bridge, void* source);
    int32_t (*create_host)(void* bridge); /* backend result */
    void* (*host)(void* bridge);
    int32_t (*resolve_graphics)(void* bridge);
    uint32_t (*tearing_supported)(void* bridge);
    uint32_t (*fsr4_supported)(void* bridge, const char* directory);
    /* One owned COM reference, if the API wrote an output. */
    int32_t (*create_interop_queue)(void* device, void** queue);
    int32_t (*create_completion_fence)(void* device, void** fence);
    /* One owned, unnamed auto-reset event, or null. The duplicate belongs to the caller. */
    void* (*create_event)(uint32_t initially_signaled);
    void* (*duplicate_event)(void* event);
    void (*close_event)(void* event);
    int32_t (*create_upload_fence)(void* bridge);
    /* One allocator and closed list on the graphics device, as two owned COM references.
       On failure the leaf has released both and the outputs are null. */
    int32_t (*create_slot)(void* bridge, void** allocator, void** list);
    const rsf_shared_surface_platform* (*shared_surface_platform)(void);
    int32_t (*create_render_buffer)(void* bridge, uint32_t width, uint32_t height,
        uint32_t format, void** out); /* owned COM reference */

    /* Creation returns an owned IDXGISwapChain4 reference. The plain creation queue
       is borrowed from the initialized facade/host. Callers lease the physical
       chain and its creation queue for the remaining operations. */
    int32_t (*create_plain)(void* bridge, const rsf_d3d11p_plain_plan*, void** physical, void** queue);
    int32_t (*query_chain4)(void* chain, void** physical);
    int32_t (*get_fullscreen)(void* physical, int32_t* fullscreen);
    int32_t (*set_fullscreen)(void* physical, int32_t fullscreen, void* target);
    int32_t (*physical_present)(void* physical, uint32_t sync, uint32_t flags);
    int32_t (*resize_physical)(void* bridge, void* physical,
        uint32_t count, uint32_t width, uint32_t height, uint32_t format);

    /* Frame and slot GPU work. */
    /* Synchronization resources and slot pairs are borrowed from the Rust owner. */
    int32_t (*join)(void* signaller, void* waiter, void* fence, uint64_t value);
    /* Waits for the slot value, resets allocator and list. *step is 0, 1 or 2. */
    int32_t (*slot_reset)(void* fence, void* event, void* allocator, void* list, uint64_t value, uint32_t* step);
    int32_t (*upload_wait)(void* bridge, void* queue, uint64_t value);
    int32_t (*acquire_backbuffer)(void* physical, void** out); /* owned COM reference */
    void (*record_copy)(void* bridge, void* list, void* backbuffer, void* source);
    void* (*context11)(void* bridge);
    /* Closes the slot list and executes command, the list the queue takes (command_list). */
    int32_t (*close_execute)(void* queue, void* list, void* command);
    int32_t (*upload_return)(void* bridge, void* queue, uint64_t value);
    int32_t (*signal_slot)(void* queue, void* fence, uint64_t value);
    /* A null fence signals the event immediately. */
    int32_t (*application_ready)(void* event, void* fence, uint64_t value);
    void (*report_device_failure)(void* bridge, int32_t result, uint32_t backend);
    void (*log_present_failure)(void* bridge, const char* step, int32_t result, uint32_t render_index);

    /* Application GetBuffer. */
    int32_t (*query_buffer_interface)(void* resource, const void* iid, void** out);
    int32_t (*get_physical_buffer)(void* physical, uint32_t index, const void* iid, void** out);
    void (*log_buffer_refusal)(void* bridge, uint32_t index, const void* iid, int32_t result,
        uint32_t physical_pending);

    /* Drain, resize, retirement and destruction. */
    int32_t (*signal_wait)(void* queue, void* fence, void* event, uint64_t value);
    int32_t (*wait_fence)(void* fence, void* event, uint64_t value);
    uint32_t (*buffer_referenced)(void* resource);
    void (*set_description)(void* bridge, uint32_t count, uint32_t width, uint32_t height,
        uint32_t format, uint32_t flags);
    int32_t (*create_retirement_fence)(void* bridge, void** out); /* transfers one owned COM reference */
    int32_t (*retire_submit)(void* bridge, void* queue, void* retirement_fence, void* vendor_fence,
        uint64_t vendor_value, void* list, uint64_t value);
    /* Retains the facade's GPU references. Rust retains its synchronization resources and slots. */
    void (*retain_all)(void* bridge);
    void (*release_presentation)(void* bridge);
    /* Preserve the old member release order around the Rust-owned synchronization references. */
    void (*release_direct_graphics)(void* bridge);
    void (*release_host)(void* bridge);
    void* (*source_queue)(void* bridge, void* interop_queue); /* returns one owned COM reference */
    /* Queue may be null while the active facade awaits physical creation. */
    void* (*command_list)(void* bridge, void* queue, void* list);
    /* Scoped creation: canonical identity of the captured inner owner and outer chain. */
    int32_t (*verify_outer)(void* captured, void* window, void* device, void* context, void* outer);
} rsf_d3d11p_platform;

/* Rust owner. Not exported from any DLL. */
/* 0 for Off or an unknown id, otherwise RSF_D3D11P_FAMILY_*. The only backend mapping. */
uint32_t rsf_core_fg_provider_family(uint32_t backend);
/* rsf_fg_get_provider's implementation, shared with the orchestrator. */
const rsf_generation_provider* rsf_d3d11p_provider_for(uint32_t backend);
const rsf_d3d11_present_setup* rsf_core_d3d11p_settings(void);
int rsf_core_d3d11p_prepare(const rsf_d3d11_present_setup* setup, uint32_t installed);
void rsf_core_d3d11p_set_scoped(uint32_t scoped);
uint32_t rsf_core_d3d11p_capture(void* window);
void rsf_core_d3d11p_captured(const rsf_d3d11p_platform* platform, void* bridge);
int32_t rsf_core_d3d11p_create(const rsf_d3d11p_platform* platform, void* factory, void* source,
    const rsf_d3d11p_request* request, const void* desc, void** out);
int32_t rsf_core_d3d11p_creation_begin(const rsf_d3d11p_platform* platform, void* window, uint64_t* token,
    uint32_t installed);
/* The published facade, or null. Comparison only; never dereferenced. */
void* rsf_core_d3d11p_active_bridge(void);
int32_t rsf_core_d3d11p_creation_end(const rsf_d3d11p_platform* platform, void* window, uint64_t token,
    void* device, void* context, void* outer);

/* Facade methods. */
void rsf_core_d3d11p_chain_destroy(rsf_d3d11p_chain* chain);
/* The physical chain is borrowed until its lease is released. When initialize is
   zero, this only looks up the published pair and never triggers lazy creation. */
int32_t rsf_core_d3d11p_acquire_physical(rsf_d3d11p_chain* chain, uint32_t initialize,
    const rsf_d3d11p_physical_lease** lease, void** physical);
void rsf_core_d3d11p_release_physical(const rsf_d3d11p_physical_lease* lease);
int32_t rsf_core_d3d11p_get_buffer(rsf_d3d11p_chain* chain, uint32_t index, const void* iid, void** out);
int32_t rsf_core_d3d11p_present(rsf_d3d11p_chain* chain, uint32_t sync, uint32_t flags);
int32_t rsf_core_d3d11p_set_fullscreen(rsf_d3d11p_chain* chain, int32_t fullscreen, void* target);
int32_t rsf_core_d3d11p_resize(rsf_d3d11p_chain* chain, uint32_t count, uint32_t width, uint32_t height,
    uint32_t format, uint32_t flags);
uint32_t rsf_core_d3d11p_faulted(rsf_d3d11p_chain* chain);
uint32_t rsf_core_d3d11p_physical_pending(rsf_d3d11p_chain* chain);
uint32_t rsf_core_d3d11p_render_index(rsf_d3d11p_chain* chain);
uint32_t rsf_core_d3d11p_application_presents(rsf_d3d11p_chain* chain);
int32_t rsf_core_d3d11p_set_max_latency(rsf_d3d11p_chain* chain, uint32_t value);
int32_t rsf_core_d3d11p_max_latency(rsf_d3d11p_chain* chain, uint32_t* value);
void* rsf_core_d3d11p_duplicate_application_ready(rsf_d3d11p_chain* chain);

/* Public bridge exports. */
void* rsf_core_d3d11p_host(void);
int rsf_core_d3d11p_graphics(rsf_streamline_graphics* graphics);
void* rsf_core_d3d11p_queue(const rsf_d3d11p_platform* platform);
void* rsf_core_d3d11p_source_queue(const rsf_d3d11p_platform* platform);
void* rsf_core_d3d11p_command_list(const rsf_d3d11p_platform* platform, void* list);
int rsf_core_d3d11p_is_owner(const rsf_d3d11p_platform* platform, void* swapchain);
/* 0 not the active facade, 1 admitted, 2 the facade of a refused scoped transaction. */
void rsf_core_d3d11p_dispatch_owned(rsf_d3d11_chain_dispatch* request,
    uint32_t (*release_owned)(void*), int32_t (*invoke_owned)(rsf_d3d11_chain_dispatch*));
int rsf_core_d3d11p_has_owner(void);
const rsf_generation_provider* rsf_core_d3d11p_provider(const rsf_d3d11p_platform* platform);
int32_t rsf_core_d3d11p_status_get(rsf_d3d11_present_status* status);
void* rsf_core_d3d11p_session(void);
uint32_t rsf_core_d3d11p_backend(void);
uint32_t rsf_core_d3d11p_backend_choices(void);
int32_t rsf_core_d3d11p_request(uint32_t backend);
int32_t rsf_core_d3d11p_switch_result(void);
uint64_t rsf_core_d3d11p_generation(void);
int32_t rsf_core_d3d11p_retire(const rsf_d3d11p_platform* platform, void* session, void* list, void** fence, uint64_t* value);
int32_t rsf_core_d3d11p_begin(const rsf_d3d11p_platform* platform, uint64_t id, uint64_t* generation);
int32_t rsf_core_d3d11p_pace(const rsf_d3d11p_platform* platform, uint64_t id, uint32_t owner,
    const uint64_t* expected, uint64_t* generation);
int32_t rsf_core_d3d11p_marker(const rsf_d3d11p_platform* platform, uint64_t generation, uint64_t id,
    uint32_t marker, uint32_t controller);
int32_t rsf_core_d3d11p_acquire(const rsf_d3d11p_platform* platform, uint64_t id);
int32_t rsf_core_d3d11p_receipt(const rsf_d3d11p_platform* platform, uint64_t generation, uint64_t id,
    uint32_t kinds, uint32_t message);
int32_t rsf_core_d3d11p_input(const rsf_d3d11p_platform* platform, uint64_t id, uint32_t kinds, uint32_t message);
int32_t rsf_core_d3d11p_abort(const rsf_d3d11p_platform* platform, uint64_t id);

#ifdef __cplusplus
}
#endif
