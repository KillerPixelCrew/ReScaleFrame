/* SPDX-License-Identifier: GPL-3.0-only */
/* Vendor-neutral C contracts for backend capability queries, super-resolution evaluation, and
   frame generation. Each adapter translates these records to its vendor SDK and reports unsupported
   combinations explicitly. */

#ifndef RSF_BACKEND_H
#define RSF_BACKEND_H

#include <rescaleframe/game_frame.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_BACKEND_ABI_VERSION 3u

/** Stable provider-family identifiers. These are C ABI values, not Rust enum discriminants. */
typedef uint32_t rsf_vendor;
#define RSF_VENDOR_NONE ((rsf_vendor)0)
#define RSF_VENDOR_NVIDIA ((rsf_vendor)1)
#define RSF_VENDOR_INTEL ((rsf_vendor)2)
#define RSF_VENDOR_AMD ((rsf_vendor)3)

/** Supported API bitmask in capabilities; select one API when opening a session. */
typedef uint32_t rsf_gfx_api;
#define RSF_API_D3D11 0x1u
#define RSF_API_D3D12 0x2u
#define RSF_API_VULKAN 0x4u

/** Requested reconstruction quality. Providers may refuse unsupported levels. */
typedef uint32_t rsf_quality;
#define RSF_QUALITY_NATIVE ((rsf_quality)0)
#define RSF_QUALITY_QUALITY ((rsf_quality)1)
#define RSF_QUALITY_BALANCED ((rsf_quality)2)
#define RSF_QUALITY_PERFORMANCE ((rsf_quality)3)
#define RSF_QUALITY_ULTRA_PERFORMANCE ((rsf_quality)4)
/* Additional quality level used by the XeSS adapter; FSR and DLSS planning reject it. */
#define RSF_QUALITY_ULTRA_QUALITY ((rsf_quality)5)

/** Vendor tag lifetime. ONLY_NOW bounds vendor use to the work recorded by the call;
 * UNTIL_NEXT_PRESENT permits later interpolation reads. Neither value removes the caller's GPU
 * synchronization obligations: textures must remain alive until submitted work and vendor reads
 * complete. supported_lifetimes uses one bit per lifetime value.
 */
typedef uint32_t rsf_lifetime;
#define RSF_LIFETIME_ONLY_NOW ((rsf_lifetime)0)
#define RSF_LIFETIME_UNTIL_NEXT_PRESENT ((rsf_lifetime)1)

/* Which surfaces a frame generator wants for the interface, as a bitmask of what it accepts. The
   three vendors name these differently and mean the same things. */
typedef uint32_t rsf_ui_mode;
/* Interpolate the completed scene without separate interface handling. */
#define RSF_UI_MODE_NONE 0x1u
/* A premultiplied interface layer, which the generator composites onto every frame it makes. */
#define RSF_UI_MODE_UI_LAYER 0x2u
/* The finished frame plus a HUD-less copy, from which the generator works out the interface. */
#define RSF_UI_MODE_BACKBUFFER_HUDLESS 0x4u
/* Both a HUD-less scene and a premultiplied interface layer. */
#define RSF_UI_MODE_BACKBUFFER_HUDLESS_UI 0x8u

typedef uint32_t rsf_latency_mode;
#define RSF_LATENCY_NONE ((rsf_latency_mode)0)
#define RSF_LATENCY_XELL ((rsf_latency_mode)1)
#define RSF_LATENCY_PCL ((rsf_latency_mode)2)

/** SR execution route for a D3D11 game. A shared D3D12 Streamline FG session requires bridge SR;
 * providers offering only D3D12 SR also use that route.
 */
typedef uint32_t rsf_sr_route;
#define RSF_SR_ROUTE_NONE ((rsf_sr_route)0)
#define RSF_SR_ROUTE_NATIVE_D3D11 ((rsf_sr_route)1)
#define RSF_SR_ROUTE_BRIDGE_D3D12 ((rsf_sr_route)2)

typedef int32_t rsf_backend_result;
#define RSF_BACKEND_OK ((rsf_backend_result)0)
#define RSF_BACKEND_ERROR_INVALID_ARGUMENT ((rsf_backend_result)-1)
#define RSF_BACKEND_ERROR_ABI_MISMATCH ((rsf_backend_result)-2)
/* This provider was built without its SDK implementation. */
#define RSF_BACKEND_ERROR_NOT_COMPILED ((rsf_backend_result)-3)
/* The runtime could not be loaded; a log callback may provide the path and platform error. */
#define RSF_BACKEND_ERROR_LOAD_FAILED ((rsf_backend_result)-4)
#define RSF_BACKEND_ERROR_MISSING_ENTRY_POINT ((rsf_backend_result)-5)
#define RSF_BACKEND_ERROR_INIT_FAILED ((rsf_backend_result)-6)
/* The SDK does not support the requested device, version, or policy. */
#define RSF_BACKEND_ERROR_NOT_SUPPORTED ((rsf_backend_result)-7)
#define RSF_BACKEND_ERROR_NOT_READY ((rsf_backend_result)-8)
#define RSF_BACKEND_ERROR_FEATURE_FAILED ((rsf_backend_result)-9)
/* Resource generations or dimensions no longer match the submitted frame/session. */
#define RSF_BACKEND_ERROR_STALE_RESOURCES ((rsf_backend_result)-10)
/* Rebuild the session or presentation chain before applying this change. */
#define RSF_BACKEND_ERROR_NEEDS_RESTART ((rsf_backend_result)-11)
/* The supplied graphics API or device does not match the provider's session. */
#define RSF_BACKEND_ERROR_WRONG_API ((rsf_backend_result)-12)

/** Optional diagnostic sink. message is UTF-8 and borrowed for the callback only.
 * Vendor callbacks can invoke it from SDK threads; the sink must be thread safe and outlive the
 * session. Do not call back into the provider while handling a diagnostic.
 */
typedef void (*rsf_backend_log_fn)(void* user, const char* message);

/* What the orchestrator knows when it asks a backend whether it can work here. */
typedef struct rsf_backend_probe_desc {
    uint32_t struct_size;
    uint32_t abi_version;
    /* The API the game itself renders with. */
    rsf_gfx_api game_api;
    /* Which adapter, so a backend can refuse hardware it does not run on without creating anything.
       Two 32 bit halves rather than a LUID, so this header needs no Windows types. */
    uint32_t adapter_luid_low;
    int32_t adapter_luid_high;
    /* Borrowed devices, either nullable. Some probes report build capability only; opening the
       feature establishes actual adapter/runtime support. */
    void* d3d11_device;
    void* d3d12_device;
    /* Where the vendor runtime is, as UTF-8. Loaded by absolute path so that a library beside the
       game's executable cannot answer instead. */
    const char* runtime_directory_utf8;
    rsf_backend_log_fn log;
    void* log_user;
} rsf_backend_probe_desc;

/** Capability output. Set struct_size before probe. Available can describe compiled capability;
 * consult the provider's open/create result for actual runtime and device support.
 */
typedef struct rsf_backend_caps {
    uint32_t struct_size;
    rsf_vendor vendor;
    /* Immutable, plugin-owned, valid until the module unloads. */
    const char* name;
    /* Non-zero when at least one feature is advertised. A refusal may explain zero availability. */
    uint32_t available;
    /* Independent supported API masks for reconstruction and generation. */
    rsf_gfx_api sr_apis;
    rsf_gfx_api fg_apis;
    /* Non-zero when the backend derives camera motion itself and does not need it supplied. */
    uint32_t reconstructs_camera_motion;
    uint32_t needs_exposure;
    /* Zero when this backend does not generate frames. Above one means multi frame generation. */
    uint32_t max_generated_frames;
    rsf_ui_mode ui_modes;
    /* Bit i denotes support for rsf_lifetime value i. */
    uint32_t supported_lifetimes;
    /* Non-zero when FG must create or wrap the presentation chain. */
    uint32_t fg_owns_swapchain;
    /* Non-zero when reconstruction and generation must share one session, as Streamline requires,
       so choosing this vendor for one constrains the other. */
    uint32_t sr_fg_share_session;
    rsf_latency_mode latency_modes;
    /* Non-zero when FG requires the source frame's CPU-to-Present latency markers. */
    uint32_t fg_requires_latency_markers;
    /* Non-zero when the number of generated frames can change without rebuilding the chain. */
    uint32_t supports_dynamic_fg;
    /* Nullable immutable module-owned diagnostic; valid until module unload. */
    const char* refusal_utf8;
} rsf_backend_caps;

/** Borrowed GPU resource and active pixel rectangle. The caller supplies the current D3D12 state,
 * performs required transitions, and holds the resource through its GPU/vendor retirement.
 */
typedef struct rsf_backend_resource {
    uint32_t struct_size;
    /* An `ID3D11Resource*` or `ID3D12Resource*`, matching the API the call is made on. */
    void* resource;
    /* A `D3D12_RESOURCE_STATES` value. Ignored on D3D11, which tracks its own. */
    uint32_t state;
    /* The region actually used, which is not always the whole surface: Unreal renders into a
       sub-rectangle of a pooled target, so at a reduced scale the surface is larger than the view. */
    uint32_t x;
    uint32_t y;
    uint32_t width;
    uint32_t height;
    /* DXGI_FORMAT value for Direct3D resources. */
    uint32_t format;
    rsf_lifetime lifetime;
    /* Must equal the submitted frame record's resource_generation. */
    uint32_t generation;
} rsf_backend_resource;

/** SR context configuration. Initialize size/version. The device and diagnostic sink must remain
 * valid until close; configuration dimensions are pixels. open returns a provider-owned opaque
 * session, released only by that provider's close callback.
 */
typedef struct rsf_sr_open_desc {
    uint32_t struct_size;
    uint32_t abi_version;
    rsf_gfx_api api;
    void* device;
    uint32_t output_width;
    uint32_t output_height;
    rsf_quality quality;
    /* Non-zero when render dimensions may change without recreating the feature. */
    uint32_t dynamic_resolution;
    uint32_t hdr;
    uint32_t inverted_depth;
    uint32_t motion_jittered;
    uint32_t auto_exposure;
    rsf_backend_log_fn log;
    void* log_user;
    /* Absolute SDK runtime directory. Borrowed during open. */
    const char* runtime_directory_utf8;
    /* AMD family: 2, 3, or 4. Never silently substitutes another family. */
    uint32_t fsr_major;
    /* Provider-specific opaque ID. Zero permits default selection; for FSR this chooses the
       newest exposed minor/patch of fsr_major. Require an exact ID when non-zero. */
    uint64_t version_id;
    uint32_t depth_infinite;
} rsf_sr_open_desc;

/** SR inputs for one evaluation. Metadata is borrowed during the call; GPU resources remain
 * leased through command completion. Required textures use ONLY_NOW and the frame's generation.
 * Color/depth/motion cover render size; output covers output size. exposure is optional 1x1.
 */
typedef struct rsf_sr_frame {
    uint32_t struct_size;
    const rsf_frame_record* record;
    rsf_backend_resource color;
    rsf_backend_resource depth;
    rsf_backend_resource motion;
    rsf_backend_resource exposure;
    rsf_backend_resource output;
    /* Pixels of the render extent, sign already applied. */
    float jitter_x;
    float jitter_y;
    /* What multiplies the stored motion to reach pixels of the render extent. */
    float motion_scale_x;
    float motion_scale_y;
    /* Non-zero discards temporal history for this evaluation. */
    uint32_t reset;
    /* Positive color pre-exposure and world-unit conversion supplied by the plugin. */
    float pre_exposure;
    float view_space_to_meters;
    /* Appended in ABI 3. Optional single-channel masks over the render extent; a null resource
       leaves the vendor default. Reactive is in [0,0.9] and lowers history weight where
       translucency changed the image (FSR reactive, XeSS responsive pixel mask). Transparency
       is translucent coverage in [0,1] (FSR transparency and composition). */
    rsf_backend_resource reactive;
    rsf_backend_resource transparency;
} rsf_sr_frame;

/** Legacy/common SR function table, immutable for the lifetime of its backend module.
 * Serialize operations on a session's render owner. open -> plan -> evaluate -> close is the
 * normal lifecycle; release_resources is optional between frames. Drain submitted GPU work
 * before resource release/close. No STL objects or exceptions cross these callback boundaries.
 */
typedef struct rsf_sr_provider {
    uint32_t struct_size;
    rsf_backend_result (*probe)(const rsf_backend_probe_desc* desc, rsf_backend_caps* caps);
    rsf_backend_result (*open)(const rsf_sr_open_desc* desc, void** session);
    /* Query SDK render dimensions for the session's fixed output size and requested quality. */
    rsf_backend_result (*plan)(void* session, rsf_quality quality, uint32_t* render_width,
                               uint32_t* render_height);
    /* Record evaluation on the supplied immediate D3D11 context or D3D12 command list.
       Does not submit the list, wait for completion, or restore the caller's pipeline bindings. */
    rsf_backend_result (*evaluate)(void* session, void* command_context, const rsf_sr_frame* frame);
    /* Release optional feature allocations. FSR/XeSS currently retain them until close. */
    rsf_backend_result (*release_resources)(void* session);
    void (*close)(void* session);
    /* Optional. Name is immutable, session-owned, and valid until close; caller never frees it. */
    rsf_backend_result (*get_version)(void* session, uint64_t* id, const char** name);
} rsf_sr_provider;

/** FG presentation-chain configuration. Providers may create, wrap, or upgrade the requested
 * chain. The caller uses the returned present_chain and preserves device/queue/window lifetime.
 */
typedef struct rsf_fg_swapchain_desc {
    uint32_t struct_size;
    uint32_t abi_version;
    /* The window, as an HWND. Untyped so this header needs no Windows headers. */
    void* hwnd;
    /* The D3D12 device and direct queue the vendor presents from. Frame generation is D3D12 on all
       three, so these are not optional the way the probe's are. */
    void* d3d12_device;
    void* d3d12_queue;
    /* What the game's own chain looks like, so the proxy matches it. */
    uint32_t width;
    uint32_t height;
    uint32_t format;
    uint32_t buffer_count;
    /* The most frames that may ever be generated between two rendered ones. Reserved at creation
       because every vendor allocates for it, and raising it later is NEEDS_RESTART. */
    uint32_t max_generated_frames;
    rsf_ui_mode ui_mode;
    rsf_latency_mode latency;
    uint32_t allow_tearing;
    rsf_backend_log_fn log;
    void* log_user;
} rsf_fg_swapchain_desc;

typedef struct rsf_fg_swapchain_result {
    uint32_t struct_size;
    /* What the caller presents through from now on, as an `IDXGISwapChain*`. Owned by the backend
       and released by `destroy_swapchain`. */
    void* present_chain;
    /* Actual reservation and UI mode granted by creation. */
    uint32_t effective_max_generated_frames;
    rsf_ui_mode effective_ui_mode;
} rsf_fg_swapchain_result;

/** One completed source frame, prepared before Present. Hold input leases through the source
 * GPU submission fence and the generation provider's retirement fence, when supplied.
 */
typedef struct rsf_fg_frame {
    uint32_t struct_size;
    const rsf_frame_record* record;
    /* The finished frame, and the same frame without the interface on it. The second is what the
       generator interpolates; interpolating the first drags the interface along with the scene. */
    rsf_backend_resource backbuffer;
    rsf_backend_resource hudless;
    /* Premultiplied interface layer for provider UI recomposition. */
    rsf_backend_resource ui;
    rsf_backend_resource depth;
    rsf_backend_resource motion;
    /* Scale stored motion to pixels of the tagged motion extent. */
    float motion_scale_x;
    float motion_scale_y;
    /* Zero means do not generate around this frame: a cut, a menu, a video, or a frame whose
       inputs were incomplete. Distinct from disabling generation, which costs a chain rebuild. */
    uint32_t interpolate;
    /* Requested inserted frames, excluding this source frame. */
    uint32_t generated_frames;
} rsf_fg_frame;

/** Legacy FG table retained for capability/negotiation callers. Live generation sessions use
 * rsf_generation_provider in frame_generation.h; FSR/XeSS legacy FG tables currently refuse work.
 */
typedef struct rsf_fg_provider {
    uint32_t struct_size;
    rsf_backend_result (*probe)(const rsf_backend_probe_desc* desc, rsf_backend_caps* caps);
    rsf_backend_result (*create_swapchain)(const rsf_fg_swapchain_desc* desc,
                                           rsf_fg_swapchain_result* result, void** session);
    /* Apply runtime options, or return NEEDS_RESTART when the change requires new reservations. */
    rsf_backend_result (*set_options)(void* session, uint32_t enabled, uint32_t generated_frames,
                                      rsf_ui_mode ui_mode);
    rsf_backend_result (*tag_frame)(void* session, void* command_list, const rsf_fg_frame* frame);
    /* Called after the present returns, for backends that finish their work there. */
    rsf_backend_result (*after_present)(void* session);
    rsf_backend_result (*resize)(void* session, uint32_t width, uint32_t height, uint32_t format);
    rsf_backend_result (*latency_marker)(void* session, rsf_latency_marker marker,
                                         uint64_t frame_id);
    rsf_backend_result (*latency_sleep)(void* session);
    /* Query confirmed generated frames when the provider can report them. */
    rsf_backend_result (*get_generated_count)(void* session, uint64_t* generated);
    void (*destroy_swapchain)(void* session);
} rsf_fg_provider;

/** Return immutable tables, never allocations. Symbols remain available without SDK headers;
 * those builds report NOT_COMPILED from feature operations. Do not free the returned tables.
 */
const rsf_sr_provider* rsf_dlss_sr_provider(void);
const rsf_fg_provider* rsf_dlss_fg_provider(void);
const rsf_sr_provider* rsf_fsr_sr_provider(void);
const rsf_fg_provider* rsf_fsr_fg_provider(void);
const rsf_sr_provider* rsf_xess_sr_provider(void);
const rsf_fg_provider* rsf_xess_fg_provider(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_BACKEND_H */
