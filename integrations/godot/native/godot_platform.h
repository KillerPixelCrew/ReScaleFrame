/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal boundary between the typed leaves in runtime.cpp and runtime_control.cpp and the Rust
   control owner (crate rsf-godot, native/lib.rs). Not a DLL ABI: both link into
   ReScaleFrame.Godot.Native.dll next to the same OrchestratorCore, and the rsfg_* exports in
   api.h stay C++ forwarders. native/platform.rs mirrors these layouts by hand (bindgen-cli 0.72.1
   was not available); both sides assert sizes and offsets.

   Rust owns the requested settings and their generation, the extension option schema, renderer
   admission, engine-service routing, the D3D12 session identity and paths, the status record,
   the D3D12 SR context plan and submission ledger (d3d12.rs), the Vulkan session (boot state,
   settings, generation, status, configure admission, the providers Auto may choose from and their
   resolution through the Runtime's policy, the presenter, FG and SR accounting, the
   primary window, the SR context owner in vulkan/sr.rs and the device creation owner in
   vulkan/creation.rs), and the input window owner (window.rs: admission, window procedure, input
   session, CPU event and ping routing, and the present-marker admission). The leaves hold
   godot-cpp, D3D12, DXGI, Win32, Vulkan and Runtime calls and decide nothing. The creation leaves
   are a separate table (rsf_godot_vk_creation_leaves) that vulkan_creation.cpp lends with each
   creation call, so the creation contract test links them without runtime.cpp. The compositor
   effect's leaves are another (rsf_godot_effect_leaves, effect.cpp), lent with each callback. */
#include "api.h"
#include <rescaleframe/native_cpu_service.h>
#include <stdint.h>

#define RSF_GODOT_PLATFORM_VERSION 8u

/* Mirror of rsf_native_fg_options without its header. */
typedef struct rsf_godot_fg_options {
    uint32_t mode, generated_frames, reflex_mode, frame_limit_us, debug;
} rsf_godot_fg_options;

/* rsf_fg12_install_explicit values chosen by the owner. The strings are owner-held and live for
   the process. The leaf adds its window admission and log trampolines. */
typedef struct rsf_godot_fg_install {
    uint32_t backend, max_generated_frames, runtime_switching, ui_mode;
    rsf_godot_fg_options options;
    uint32_t depth_inverted, depth_infinite;
    float units_to_meters;
    uint32_t engine_type;
    const char* fidelityfx_directory;
    const char* xess_directory;
    const char* streamline_directory;
    const char* engine_version;
    const char* project_id;
} rsf_godot_fg_install;

/* D3D12 session paths for SR context creation. Owner-held, valid for the process. */
typedef struct rsf_godot_paths {
    const char* runtime_directory;
    const char* streamline_directory;
    const char* streamline_interposer;
    const char* fidelityfx_directory;
    const char* xess_directory;
    const char* engine_version;
    const char* project_id;
} rsf_godot_paths;

/* Shared input counters, typed as the status fields they fill. */
typedef struct rsf_godot_input_facts {
    uint64_t keyboard, mouse, controller, pings, substitutes, order_violations, derived_boundaries;
} rsf_godot_input_facts;

typedef struct rsf_godot_window_facts {
    uint32_t foreground_process_id, current_process_id;
    uint32_t input_window_visible, reserved;
} rsf_godot_window_facts;

/* The owner process and class of a candidate input window, as read. process_id is zero for an
   invalid window; class_length is GetClassNameW's count into class_name, zero on failure. */
typedef struct rsf_godot_window_identity {
    uint32_t process_id, current_process_id;
    uint32_t class_length, reserved;
    uint16_t class_name[64];
} rsf_godot_window_identity;

/* rsf_fg12_status plus the shared presentation provider, typed as the status fields. */
typedef struct rsf_godot_fg_facts {
    uint32_t available, active, reason, max_generated_frames, generated_frames;
    uint32_t statistics_flags;
    int32_t last_result, switch_result;
    uint32_t backend, reserved;
    uint64_t tagged_frames, total_presented, generated_presented, source_frame_id, generated_callbacks;
    uint64_t provider_generation;
} rsf_godot_fg_facts;

/* rsf_native_latency_status, typed as the status fields. */
typedef struct rsf_godot_latency_facts {
    uint32_t available, effective_mode;
    uint32_t pcl_available, report_available, owner, reserved;
    uint64_t paced_frames, marker_calls;
    rsf_godot_input_facts inputs;
} rsf_godot_latency_facts;

/* One D3D12 SR context the owner planned: the provider (Auto already resolved by the Runtime,
   spatial mapped to the normalising context), quality, preset and extents. The leaf adds the
   bound device, the presentation host and the log. */
typedef struct rsf_godot_sr_create {
    uint32_t backend, quality, dlss_preset;
    uint32_t output_width, output_height, render_width, render_height;
    uint32_t reserved;
    rsf_godot_paths paths;
} rsf_godot_sr_create;

/* rsf_sr12_create's result and handle, and rsf_sr12_plan's extent when a context exists. The
   owner holds the handle; desired_* arrive holding the previous plan and stay when none is made. */
typedef struct rsf_godot_sr_context {
    void* handle;
    int32_t result;
    uint32_t desired_width, desired_height, reserved;
} rsf_godot_sr_context;

/* One rsf_sr12_evaluate: the command slot, the gated screen and the sharpening snapshot. */
typedef struct rsf_godot_sr_evaluate {
    uint32_t slot, screen;
    rsf_sharpening_options sharpening;
} rsf_godot_sr_evaluate;

/* Vulkan owner facts, read under its guard. The compiled flags are build facts. */
typedef struct rsf_godot_vk_facts {
    /* Formerly xess_device_ready and interop_device_ready; the creation owner (vulkan/creation.rs)
       keeps both now. Zero. */
    uint32_t reserved_creation[2];
    uint32_t graph_handoff;
    uint32_t fsr4_sr_compiled, fsr4_fg_compiled, xess_fg_compiled;
    uint32_t presenter;
    /* rsf_slvk_initialize with DLSS SR and FG succeeded at this session's start: the SDK
       availability Auto requires before it may choose DLSS. Formerly reserved. */
    uint32_t streamline_ready;
    /* rsf_fgvk_backend of the live presenter; zero without one or when the query fails. */
    uint32_t presenter_backend;
    uint32_t sdk_shutdown_pending;
} rsf_godot_vk_facts;

/* rsf_latencyvk_status, as read. */
typedef struct rsf_godot_vk_latency_facts {
    uint32_t low_latency_available, report_available, pcl_message, mode, owner, reserved;
    uint64_t sleep_calls, marker_calls;
} rsf_godot_vk_latency_facts;

/* The Vulkan session as the render, presentation and capture leaves read it under the guard. */
typedef struct rsf_godot_vk_view {
    rsf_godot_settings settings;
    uint32_t sr_backend; /* the requested SR provider, Auto resolved */
    uint64_t generation, session_id;
    uint32_t fg_backend; /* the requested FG provider, Auto resolved */
    uint32_t output_width, output_height; /* the last SR pass's output */
    int32_t fg_result;
} rsf_godot_vk_view;

/* One FG pass through the presenter in rsfg_render_vulkan. */
typedef struct rsf_godot_vk_fg_pass {
    uint32_t role, screen;
    uint64_t session_id;
    uint32_t view_id, render_width, render_height;
    /* Filled by rsf_godot_vk_fg_begin: the enable rule and the requested count. */
    uint32_t enabled, generated_frames;
    /* Filled by the leaf after rsf_fgvk_enable_options and rsf_fgvk_backend. */
    int32_t options_result;
    uint32_t backend_valid, backend;
} rsf_godot_vk_fg_pass;

/* rsf_fgvk_get_status after a presenter Present. */
typedef struct rsf_godot_vk_present_facts {
    uint32_t enabled;
    int32_t result;
    uint64_t generated_callbacks, presented, tagged;
} rsf_godot_vk_present_facts;

/* One SR pass of rsfg_render_vulkan after its FG section, as the SR context owner reads it under
   the guard. call is vulkan.cpp's private record of the packet, handoff, continuation, frame record
   and current command buffer; the owner only lends it back to the SR leaves. device and
   device_proc are compared and kept for retirement, never called by the owner. */
typedef struct rsf_godot_vk_sr_pass {
    void* call;
    void* commands;
    void* device;
    rsf_godot_vk_proc device_proc;
    uint32_t output_width, output_height;
    uint64_t source_frame_id;
    uint32_t render_width, render_height;
    /* The graph lease: a handoff, its split_record, the continuation output. */
    uint32_t handoff, split_record, continuation;
    /* Build fact: the FSR4 SR provider is compiled. */
    uint32_t fsr4_compiled;
} rsf_godot_vk_sr_pass;

/* The provider and quality of one Vulkan SR context. The leaf adds the packet's device, procs and
   camera depth and motion facts and the provider's runtime directory. */
typedef struct rsf_godot_vk_sr_create {
    uint32_t backend, quality;
} rsf_godot_vk_sr_create;

/* The created context, kept by the owner even when result is not RSF_BACKEND_OK. */
typedef struct rsf_godot_vk_sr_context {
    void* handle;
    int32_t result;
    uint32_t reserved;
} rsf_godot_vk_sr_context;

/* The Vulkan creation boundary. The creation owner (vulkan/creation.rs) keeps the resolver routes,
   the stage order XeSS, Win32 interop, core latency, the extension lists, the feature structures
   XeSS needs added, the retries, the XeSS and interop readiness and the adapter facts of the
   physical device the last created device runs on, which the session's Auto policy reads. The leaves in
   vulkan_creation.cpp keep every Vulkan structure: a request is a VkInstanceCreateInfo or
   VkDeviceCreateInfo, the engine's or one a leaf derived into the call record, and call is the
   entry point's private record for one creation call. These are not vulkan.cpp's guarded leaves
   and none takes its guard. */
#define RSF_GODOT_VK_CREATION_VERSION 2u

/* rsf_godot_vk_creation_resolve scopes and routes. FORWARD returns the resolved function. */
#define RSF_GODOT_VK_SCOPE_INSTANCE 0u /* the resolver rsfg_vk_resolver returns */
#define RSF_GODOT_VK_SCOPE_DEVICE 1u   /* the vkGetDeviceProcAddr that resolver returns */
#define RSF_GODOT_VK_ROUTE_NONE 0u
#define RSF_GODOT_VK_ROUTE_FORWARD 1u
#define RSF_GODOT_VK_ROUTE_CREATE_INSTANCE 2u
#define RSF_GODOT_VK_ROUTE_DEVICE_PROC 3u
#define RSF_GODOT_VK_ROUTE_CREATE_DEVICE 4u
#define RSF_GODOT_VK_ROUTE_DESTROY_DEVICE 5u

/* A device request's pNext chain and pEnabledFeatures, as read. */
typedef struct rsf_godot_vk_chain_facts {
    /* At most 64 structures, each of a type the clone copies, none repeated. The presence flags
       below are read only for such a chain. */
    uint32_t cloneable;
    uint32_t features2, vulkan12, vulkan13, float16_int8, scalar_block_layout, integer_dot_product;
    uint32_t enabled_features;
    /* Within the first 64 structures, timelineSemaphore set in the timeline semaphore or the
       Vulkan 1.2 features. */
    uint32_t timeline_semaphore;
    uint32_t reserved;
} rsf_godot_vk_chain_facts;

/* RSF_GODOT_VK_XESS_*: the structures the XeSS clone adds before the SDK patches it. FEATURES2 is an
   empty VkPhysicalDeviceFeatures2, ENABLED_FEATURES one holding the request's pEnabledFeatures. */
#define RSF_GODOT_VK_XESS_FEATURES2 1u
#define RSF_GODOT_VK_XESS_ENABLED_FEATURES 2u
#define RSF_GODOT_VK_XESS_FLOAT16_INT8 4u
#define RSF_GODOT_VK_XESS_SCALAR_BLOCK_LAYOUT 8u
#define RSF_GODOT_VK_XESS_INTEGER_DOT_PRODUCT 16u

/* RSF_GODOT_VK_STAGE_*: the derived device request slot of each creation stage. */
#define RSF_GODOT_VK_STAGE_XESS 0u
#define RSF_GODOT_VK_STAGE_INTEROP 1u
#define RSF_GODOT_VK_STAGE_LATENCY 2u

/* A copy of base with the given extension list, its pNext replaced when replace_next is set (also
   by null) and pEnabledFeatures cleared when clear_enabled_features is set. */
typedef struct rsf_godot_vk_device_derive {
    const void* base;
    const char* const* extensions;
    uint32_t extension_count, replace_next;
    void* next;
    uint32_t clear_enabled_features, reserved;
} rsf_godot_vk_device_derive;

/* vkGetPhysicalDeviceProperties of the physical device a created device runs on, as read:
   vendorID, deviceID, and software when deviceType is VK_PHYSICAL_DEVICE_TYPE_CPU. */
typedef struct rsf_godot_vk_adapter {
    uint32_t vendor, device, software, reserved;
} rsf_godot_vk_adapter;

/* rsf_latencyvk_require_device's result and requirements. */
typedef struct rsf_godot_vk_latency_requirements {
    int32_t result;
    uint32_t extension_count;
    const char* const* extensions;
    void* features;
} rsf_godot_vk_latency_requirements;

/* One vkCreateInstance through the resolver. request, allocator and output are the engine's;
   extensions and extension_count are request's list as read. */
typedef struct rsf_godot_vk_instance_call {
    void* call;
    const void* request;
    const void* allocator;
    void* output; /* VkInstance*, a dispatchable handle the owner reads after success */
    const char* const* extensions;
    uint32_t extension_count, reserved;
} rsf_godot_vk_instance_call;

/* One vkCreateDevice through the resolver, as rsf_godot_vk_instance_call. */
typedef struct rsf_godot_vk_device_call {
    void* call;
    void* physical_device;
    const void* request;
    const void* allocator;
    void* output; /* VkDevice* */
    const char* const* extensions;
    uint32_t extension_count, reserved;
} rsf_godot_vk_device_call;

/* Process-lived table from vulkan_creation.cpp, lent with each creation call. Every entry is
   present and catches its own exceptions. A uint32_t completion of zero or a null derived request
   means an exception ended the leaf; the owner then ends the creation call with
   VK_ERROR_OUT_OF_HOST_MEMORY as the entry point's catch did. Functions are the owner's resolved
   Vulkan entry points, passed back unchanged. */
typedef struct rsf_godot_vk_creation_leaves {
    uint32_t struct_size, version;
    /* rsf_xessvk_instance_requirements. */
    int32_t (*xess_instance_requirements)(uint32_t* count, const char* const** names, uint32_t* minimum_api);
    /* A copy of base in call whose application info (a default one when base has none) has an API
       version of at least minimum_api, with the given extension list. */
    const void* (*derive_instance)(void* call, const void* base, uint32_t minimum_api,
        const char* const* extensions, uint32_t extension_count);
    /* create is vkCreateInstance. */
    int32_t (*create_instance)(rsf_godot_vk_function create, const void* request, const void* allocator, void* output);
    void (*chain_facts)(const void* request, rsf_godot_vk_chain_facts* facts);
    /* Clones request's pNext chain into call, adds the RSF_GODOT_VK_XESS_* structures, moves
       VkPhysicalDeviceFeatures2 first and writes the head; null when the clone refused. */
    uint32_t (*xess_features)(void* call, const void* request, uint32_t structures, void** features);
    /* rsf_xessvk_device_requirements. */
    int32_t (*xess_device_requirements)(void* instance, void* physical_device, uint32_t* count,
        const char* const** names, void** features);
    /* Nonzero when enumerate (vkEnumerateDeviceExtensionProperties) lists every name for
       physical_device within 4096 entries; zero when either enumeration fails. */
    uint32_t (*device_extensions)(rsf_godot_vk_function enumerate, void* physical_device,
        const char* const* names, uint32_t count);
    /* Clones request's pNext chain into call and asks rsf_latencyvk_require_device with
       instance_proc; RSF_BACKEND_ERROR_NOT_SUPPORTED without asking when the clone refused. */
    uint32_t (*latency_requirements)(void* call, void* instance, void* physical_device,
        rsf_godot_vk_proc instance_proc, const void* request, rsf_godot_vk_latency_requirements* requirements);
    /* The stage's derived request in call. */
    const void* (*derive_device)(void* call, uint32_t stage, const rsf_godot_vk_device_derive* derive);
    /* create is vkCreateDevice. */
    int32_t (*create_device)(rsf_godot_vk_function create, void* physical_device, const void* request,
        const void* allocator, void* output);
    /* rsf_latencyvk_device_created for the device and the request that created it. */
    int32_t (*latency_created)(void* instance, void* physical_device, void* device,
        rsf_godot_vk_proc instance_proc, const void* request);
    /* rsf_latencyvk_device_destroyed. */
    void (*latency_destroyed)(void* device);
    /* destroy is the device's vkDestroyDevice. */
    void (*destroy_device)(rsf_godot_vk_function destroy, void* device, const void* allocator);
    /* properties is vkGetPhysicalDeviceProperties. Version 2. */
    uint32_t (*adapter)(rsf_godot_vk_function properties, void* physical_device, rsf_godot_vk_adapter* adapter);
} rsf_godot_vk_creation_leaves;

/* The compositor effect boundary. The effect owner (effect.rs) keeps each ReScaleFrameEffect's
   published snapshot, its callback count, the pending capture request and the last motion capture,
   the scene buffer admission, the capture admission and the motion classification, and the probe's
   published stage, reason and inactive pipeline claims. effect.cpp keeps the godot-cpp class, its
   registration and callback contract, the RenderSceneBuffersRD, RenderSceneData and
   RenderingDevice reads, the readback compute pass and the status Dictionary keys. call is the
   render callback's private record. These leaves take no guard and call no other owner. */
#define RSF_GODOT_EFFECT_VERSION 2u
/* RenderingDevice::DATA_FORMAT_R16G16_SFLOAT, the engine's velocity format and the only one the
   readback is interpreted for. effect.cpp asserts the value. */
#define RSF_GODOT_EFFECT_MOTION_FORMAT 82

/* The scene buffer facts of one POST_TRANSPARENT callback. Matrices are the scene data's real_t
   values widened to double: camera_transform is the basis rows then the origin, and
   jittered_projection the four columns. The three valid flags are texture_is_valid of layer 0. */
typedef struct rsf_godot_effect_frame {
    uint64_t render_target;
    uint32_t views;
    int32_t internal_width, internal_height, output_width, output_height;
    uint32_t color_valid, depth_valid, motion_valid;
    uint32_t reserved[2];
    double camera_transform[12];
    double jittered_projection[16];
} rsf_godot_effect_frame;

/* Read only for admitted buffers: the three layer formats (RenderingDevice::DataFormat), and
   whether the logical device, the command queue and all three native textures have handles. */
typedef struct rsf_godot_effect_resources {
    int32_t color_format, depth_format, motion_format;
    uint32_t native_device, native_queue, native_textures;
} rsf_godot_effect_resources;

/* An effect's published state, copied under the owner's lock. stage, reason and snapshot_scope are
   owner-held static strings. A field the last callback did not reach keeps its default: zero, the
   identity matrices and format -1. */
typedef struct rsf_godot_effect_status {
    uint32_t struct_size, version;
    uint64_t callbacks;
    rsf_godot_effect_frame frame;
    rsf_godot_effect_resources resources;
    uint32_t buffers_available;
    uint32_t capture_succeeded;
    uint64_t captures, capture_callback, capture_pixels, capture_moving_pixels;
    uint64_t capture_nonfinite_pixels, capture_unwritten_pixels;
    double capture_max_abs_uv;
    const char* stage;
    const char* reason;
    const char* snapshot_scope;
    uint32_t rendering_ready, sr_active, fg_active, latency_active;
} rsf_godot_effect_status;

/* Process-lived table from effect.cpp, lent with each render callback. Every entry is present and
   catches its own exceptions; a completion of zero means an exception ended the leaf. */
typedef struct rsf_godot_effect_readback {
    const char* shader;
    uint32_t bytes, groups_x, groups_y;
} rsf_godot_effect_readback;

typedef struct rsf_godot_effect_leaves {
    uint32_t struct_size, version;
    /* The scene buffers, scene data and the RenderingServer's RenderingDevice, then the frame facts
       and the layer 0 color, depth and velocity textures kept in call. Zero when any of the three
       is missing; frame is then untouched. */
    uint32_t (*frame)(void* call, rsf_godot_effect_frame* frame);
    /* The layer formats and native handle facts of the textures frame kept. */
    uint32_t (*resources)(void* call, rsf_godot_effect_resources* resources);
    /* Converts the velocity texture through the render graph into an owned storage buffer of
       internal_width * internal_height vec2 and reads it back (a GPU stall). The bytes stay in call
       until the callback ends; empty when the shader, pipeline or buffer was refused. */
    uint32_t (*read_motion)(void* call, const rsf_godot_effect_readback* plan, const uint8_t** data, uint64_t* size);
} rsf_godot_effect_leaves;

typedef struct rsf_godot_effect rsf_godot_effect;

/* rsf_godot_platform.vulkan_sr_log messages. Each leaf writes nothing without a session log. */
#define RSF_GODOT_VK_LOG_REJECTED 1u       /* provider result and spatial fallback result */
#define RSF_GODOT_VK_LOG_FSR4_DISPATCH 2u  /* the last FSR4 evaluate's code and continuation */
#define RSF_GODOT_VK_LOG_INPUT_REFUSAL 3u  /* the last evaluated frame, its plan and record */

/* Process-lived table from runtime.cpp. Every entry is present and catches its own exceptions. */
typedef struct rsf_godot_platform {
    uint32_t struct_size, version;
    uint64_t (*tick_count)(void);
    /* Creates the D3D12 frame capture owner; zero refuses the boot. */
    uint32_t (*prepare_capture)(void);
    int32_t (*fg_install)(const rsf_godot_fg_install* install);
    void (*fg_options)(const rsf_godot_fg_options* options);
    /* Header-inline Runtime validators and defaults. */
    uint32_t (*dlss_preset_valid)(uint32_t preset);
    uint32_t (*sharpening_valid)(const rsf_sharpening_options* options);
    void (*sharpening_defaults)(rsf_sharpening_options* options);
    uint32_t (*fg_backend_known)(uint32_t backend);
    /* The Vulkan owner in vulkan.cpp. vulkan_supported and vulkan_latency_facts need no guard.
       vulkan_locked runs body with the owner's guard held and returns its result, or zero when it
       could not; the guard is not reentrant. Every entry after it is called only inside such a
       body and does not take the guard itself. */
    uint32_t (*vulkan_supported)(void);
    uint32_t (*vulkan_latency_facts)(rsf_godot_vk_latency_facts* facts);
    int32_t (*vulkan_locked)(int32_t (*body)(void* user), void* user);
    /* Directories, Streamline and XeSS initialization, creation resolver and the ready log. */
    int32_t (*vulkan_start)(const rsf_godot_setup* setup);
    /* After the owner drained the presenter and retired the SR contexts: capture retirement,
       XeSS shutdown and the SDK shutdown or its deferral to presenter retirement. Zero keeps the
       session. */
    int32_t (*vulkan_stop)(void);
    /* Frame dump cancel and runtime directory release after the session ended. */
    void (*vulkan_release)(uint64_t session_id);
    void (*vulkan_facts)(rsf_godot_vk_facts* facts);
    int32_t (*vulkan_presenter_max)(uint32_t* count);
    /* FSR4 FG capacity on the presenter's device; zero refuses. */
    uint32_t (*vulkan_fsr4_fg_probe)(void);
    int32_t (*vulkan_latency_options)(uint32_t mode, uint32_t frame_limit_us);
    /* Status facts. latency_facts returns zero when the Runtime has no latency status. */
    void (*cpu_counters)(rsf_godot_input_facts* facts);
    /* The foreground process and whether window, the admitted input window or null, is visible. */
    void (*window_facts)(void* window, rsf_godot_window_facts* facts);
    void (*fg_facts)(rsf_godot_fg_facts* facts);
    uint32_t (*latency_facts)(rsf_godot_latency_facts* facts);
    /* The D3D12 SR context and submission leaves (d3d12.rs), called on Godot's render thread.
       A uint32_t completion of zero means an exception ended the leaf; the owner then refuses
       the call as the export's catch did. */
    /* Keeps the engine's first device and queue and creates the fence; zero when the fence
       failed, which keeps the borrow as before. */
    uint32_t (*d3d12_bind)(void* device, void* queue);
    /* Nonzero when the fence reached value within 15 s, or value is zero, or no fence exists. */
    uint32_t (*d3d12_wait)(uint64_t value);
    /* Queue Signal of value; zero without a fence or on failure. */
    uint32_t (*d3d12_signal)(uint64_t value);
    /* The Runtime's Auto SR and FG provider for the bound adapter. */
    uint32_t (*sr_default_backend)(void);
    uint32_t (*fg_default_backend)(void);
    /* Nonzero when the presentation provider has an owner; writes its backend. */
    uint32_t (*present_owner)(uint32_t* backend);
    void (*present_request)(uint32_t backend);
    uint32_t (*sr_create)(const rsf_godot_sr_create* request, rsf_godot_sr_context* context);
    void (*sr_destroy)(void* context);
    /* Sharpening snapshot, then rsf_sr12_evaluate of a copy of pass with request->screen. */
    uint32_t (*sr_evaluate)(void* context, void* commands, const rsf_game_render_pass* pass,
        const rsf_godot_sr_evaluate* request, int32_t* result);
    /* Logs the refused pass, its resources and the planned extent. */
    void (*sr_refusal_log)(const rsf_game_render_pass* pass, uint32_t desired_width, uint32_t desired_height);
    /* The "native-sr-begin" capture event when the capture wants the frame. */
    uint32_t (*capture_sr_begin)(const rsf_game_render_pass* pass);
    /* Capture flush of slot, the HUD-less colour record and rsf_fg12_hudless. */
    uint32_t (*final_scene)(void* commands, const rsf_game_render_pass* pass, uint32_t slot);
    void (*fg_submitted)(uint64_t session_id, uint64_t frame_id);
    /* rsf_fg12_render_begin and its "render-submit-begin" capture event. */
    uint32_t (*render_begin)(uint64_t session_id, uint64_t frame_id);
    void (*present_window)(const rsf_game_render_pass* pass);
    uint32_t (*fg_end_session)(uint64_t session_id);
    /* Capture flush, context destroy (null accepted) and the session's frame dump cancel. */
    uint32_t (*d3d12_release)(void* context, uint64_t session_id);
    /* The Vulkan SR context leaves (vulkan/sr.rs), called only while vulkan.cpp's guard is held:
       inside a vulkan_locked body or from rsf_godot_vk_sr_render. None takes the guard. */
    /* rsf_fgvk_drain of the live presenter, after rsf_fgvk_enable(0) when disable is nonzero;
       RSF_BACKEND_OK without a presenter. */
    int32_t (*vulkan_presenter_drain)(uint32_t disable);
    /* vkDeviceWaitIdle through device_proc; zero without the entry point or on failure. */
    uint32_t (*vulkan_device_idle)(void* device, rsf_godot_vk_proc device_proc);
    int32_t (*vulkan_sr_destroy)(void* context);
    /* rsf_fsr4vk_destroy's bridge code; zero is success. */
    int32_t (*vulkan_fsr4_destroy)(void* context);
    /* The calls below take the pass's call record. A uint32_t completion of zero means an
       exception ended the leaf; the owner then refuses the pass as the export's catch did. */
    /* rsf_srvk_create, and the FSR4 interop context with the pass's handoff and its log. */
    uint32_t (*vulkan_sr_create)(void* call, const rsf_godot_vk_sr_create* request, rsf_godot_vk_sr_context* created);
    uint32_t (*vulkan_fsr4_create)(void* call, const rsf_godot_vk_sr_create* request, rsf_godot_vk_sr_context* created);
    uint32_t (*vulkan_dlss_preset)(void* context, uint32_t preset);
    /* The context's planned input extent; zero when the provider does not answer. */
    uint32_t (*vulkan_sr_plan)(void* call, void* context, uint32_t* width, uint32_t* height);
    uint32_t (*vulkan_fsr4_plan)(void* call, void* context, uint32_t* width, uint32_t* height);
    /* The frame from the packet with the owner's sharpening, then the provider evaluate. The FSR4
       leaf continues the call and the export's continuation on next. */
    uint32_t (*vulkan_sr_evaluate)(void* call, void* context, const rsf_sharpening_options* sharpening,
        int32_t* result);
    uint32_t (*vulkan_fsr4_evaluate)(void* call, void* context, const rsf_sharpening_options* sharpening,
        int32_t* result, void** next);
    /* rsf_srvk_spatial_fallback of the call's colour into its output on its current commands. */
    uint32_t (*vulkan_sr_fallback)(void* call, int32_t* result);
    /* One RSF_GODOT_VK_LOG_* message. */
    uint32_t (*vulkan_sr_log)(void* call, uint32_t kind, int32_t provider_result, int32_t fallback_result);
    /* rsf_input_dump_result of the call's record for the "sr" stage. */
    uint32_t (*vulkan_sr_dump_result)(void* call, uint32_t backend, int32_t result);
    /* The window and input leaves (window.rs). Windows are handles passed through; a procedure is
       a WNDPROC as GWLP_WNDPROC stores it. window_forward runs on the window's thread inside the
       owner's procedure and may re-enter it. */
    /* GetWindowThreadProcessId, GetCurrentProcessId and GetClassNameW into 64 characters. */
    void (*window_identity)(void* window, rsf_godot_window_identity* identity);
    /* GetWindowLongPtrW(GWLP_WNDPROC); zero on failure. */
    intptr_t (*window_procedure_get)(void* window);
    /* SetWindowLongPtrW(GWLP_WNDPROC): the replaced procedure, zero on failure. */
    intptr_t (*window_procedure_set)(void* window, intptr_t procedure);
    /* CallWindowProcW of previous, or DefWindowProcW when previous is zero. */
    intptr_t (*window_forward)(intptr_t previous, void* window, uint32_t message, uintptr_t wparam,
        intptr_t lparam);
    /* QueryPerformanceCounter and QueryPerformanceFrequency. */
    void (*query_counter)(uint64_t* counter, uint64_t* frequency);
    /* Endpoints supplied by the Runtime graphics session at setup. */
    const rsf_cpu_session_service* (*d3d12_cpu_service)(void);
    const rsf_cpu_session_service* (*vulkan_cpu_service)(void);
    /* rsf_latencyvk_present. */
    void (*vulkan_present_marker)(uint32_t begin);
    /* Version 6. The Runtime's Auto policy for an adapter and the providers a host can serve (bit n
       = provider n): rsf_gpu_select_sr_backend, and for FG rsf_fg_default_backend's preference
       through the Runtime's private rsf_fg_select_auto_backend (fg_auto_choice.h). */
    uint32_t (*sr_select_backend)(uint32_t vendor, uint32_t software, uint32_t choices);
    uint32_t (*fg_select_backend)(uint32_t vendor, uint32_t software, uint32_t choices);
} rsf_godot_platform;

/* Extension option schema. The leaf reads each named Dictionary entry once, with the conversion
   the field names; the owner validates and composes the settings. */
#define RSF_GODOT_OPTION_COUNT 21u
#define RSF_GODOT_OPTION_INTEGER 0u
#define RSF_GODOT_OPTION_REAL 1u
#define RSF_GODOT_VARIANT_OTHER 0u
#define RSF_GODOT_VARIANT_INT 1u
#define RSF_GODOT_VARIANT_BOOL 2u
typedef struct rsf_godot_option_field {
    const char* name;
    uint32_t conversion;
    uint32_t reserved;
} rsf_godot_option_field;
typedef struct rsf_godot_option_value {
    uint32_t present, variant;
    int64_t integer;
    double real;
} rsf_godot_option_value;

#define RSF_GODOT_ADMIT_DRIVER 1u
#define RSF_GODOT_ADMIT_RENDERER 2u

#ifdef __cplusplus
extern "C" {
#endif
/* runtime.cpp: the table runtime_control.cpp passes to the owner. */
const rsf_godot_platform* rsf_godot_platform_leaves(void);

/* Rust owner. platform is the process-lived table above; the first accepted one is kept. */
int rsf_godot_boot(const rsf_godot_platform* platform, const rsf_godot_setup* setup);
int rsf_godot_configure(const rsf_godot_platform* platform, const rsf_godot_settings* settings);
void rsf_godot_log(const char* message);

/* D3D12 session state read by the rsfg_* forwarders in runtime.cpp. */
int rsf_godot_booted(void);

/* D3D12 render, render-begin and shutdown leaves reached from the public forwarders. */
int rsf_godot_d3d12_render(const rsf_godot_platform* platform, void* device, void* queue, void* commands,
    const rsf_game_render_pass* pass);
void rsf_godot_d3d12_render_begin(const rsf_godot_platform* platform, uint64_t source_frame_id);
int rsf_godot_d3d12_shutdown(const rsf_godot_platform* platform);

/* Vulkan session (vulkan.rs). Boot and shutdown take the guard through vulkan_locked; the
   remaining calls are made by vulkan.cpp while it holds the guard. */
int rsf_godot_vk_booted(void);
int rsf_godot_boot_vulkan(const rsf_godot_platform* platform, const rsf_godot_setup* setup);
/* The caller found the session booted. */
int rsf_godot_shutdown_vulkan(const rsf_godot_platform* platform);
/* Fills view and returns whether the session is booted. Auto is resolved through the Runtime's
   policy leaves of the table the boot registered; before any boot nothing is written. */
int rsf_godot_vk_view_get(rsf_godot_vk_view* view);
void rsf_godot_vk_presenter_created(uint32_t ready, uint32_t backend, uint32_t max_generated);
/* Zero: the engine must drain and recreate its WSI owner, also when Auto now resolves to
   another provider. */
int rsf_godot_vk_presenter_current(uint32_t presenter_backend);
void rsf_godot_vk_presented(const rsf_godot_vk_present_facts* facts);
void rsf_godot_vk_presenter_retired(void);
void rsf_godot_vk_fg_begin(rsf_godot_vk_fg_pass* pass);
/* Nonzero: record the pass. Zero stored the refusal as the FG result. */
int rsf_godot_vk_fg_admit(const rsf_godot_vk_fg_pass* pass);
void rsf_godot_vk_fg_recorded(const rsf_godot_vk_fg_pass* pass, int32_t result);
/* The SR section of rsfg_render_vulkan: context retirement, creation, plan, evaluate, the spatial
   fallback and the SR accounting. Returns the export's result. */
int rsf_godot_vk_sr_render(const rsf_godot_platform* platform, const rsf_godot_vk_sr_pass* pass);
/* The live SR context handles, null when none, for rsfg_vk_recording_begin. */
void rsf_godot_vk_sr_contexts(void** context, void** fsr4_context);
/* The primary window rsfg_vk_window last reported since the session's start, or null. */
void* rsf_godot_vk_primary_window(void);

/* Vulkan creation owner (vulkan/creation.rs), reached from vulkan_creation.cpp without the guard:
   from the start and stop leaves and from the resolver's entry points on the engine's thread. */
/* Keeps underlying (Streamline's or the loader's resolver), whether XeSS requirements are
   prepared and the session log, and clears both readiness facts. The adapter facts, like the
   owner instance, describe the live device and survive. Nonzero when underlying is set. */
int rsf_godot_vk_creation_begin(rsf_godot_vk_proc underlying, uint32_t xess, rsf_game_log_fn log, void* log_user);
/* Ends XeSS preparation and clears both readiness facts. The resolver keeps forwarding. */
void rsf_godot_vk_creation_stop(void);
/* The last device was created with the XeSS, or the Win32 interop, requirements. */
int rsf_godot_vk_xess_ready(void);
int rsf_godot_vk_interop_ready(void);
/* An RSF_GODOT_VK_ROUTE_*; forwarded receives the function for ROUTE_FORWARD, otherwise null. */
uint32_t rsf_godot_vk_creation_resolve(uint32_t scope, void* object, const char* name,
    rsf_godot_vk_function* forwarded);
/* The VkResult of the intercepted vkCreateInstance, vkCreateDevice. */
int32_t rsf_godot_vk_create_instance(const rsf_godot_vk_creation_leaves* leaves, const rsf_godot_vk_instance_call* call);
int32_t rsf_godot_vk_create_device(const rsf_godot_vk_creation_leaves* leaves, const rsf_godot_vk_device_call* call);
/* The intercepted vkDestroyDevice. */
void rsf_godot_vk_destroy_device(const rsf_godot_vk_creation_leaves* leaves, void* device, const void* allocator);

/* Compositor effect owner (effect.rs), one per ReScaleFrameEffect instance. create and destroy run
   in the class constructor and destructor; request_capture and status on the caller's thread;
   render in _render_callback on the render thread, after the class's callback type check. */
rsf_godot_effect* rsf_godot_effect_create(void);
void rsf_godot_effect_destroy(rsf_godot_effect* effect);
void rsf_godot_effect_request_capture(rsf_godot_effect* effect);
void rsf_godot_effect_render(rsf_godot_effect* effect, const rsf_godot_effect_leaves* leaves, void* call);
/* Nonzero when the record's header matched and it was filled. */
int rsf_godot_effect_status_get(const rsf_godot_effect* effect, rsf_godot_effect_status* status);

/* Input window owner (window.rs). */
/* The Runtime's accept_window question: nonzero for the admitted window, the first window of this
   process with the Engine class. */
int rsf_godot_accept_window(const rsf_godot_platform* platform, void* window);
/* The window part of rsfg_shutdown: the checked procedure restore and the end of the admission
   and input session. */
void rsf_godot_window_release(const rsf_godot_platform* platform);
/* rsfg_vk_present_marker: nonzero when the admitted window's marker was reported. */
int rsf_godot_vk_present_marker(const rsf_godot_platform* platform, uint64_t window, uint32_t begin);

/* Extension control (runtime_control.cpp). */
const rsf_godot_option_field* rsf_godot_option_fields(uint32_t* count);
int rsf_godot_compose_settings(const rsf_godot_platform* platform, const rsf_godot_option_value* values,
    uint32_t count, rsf_godot_settings* settings);
/* Null names are unsupported. Returns RSF_GODOT_ADMIT_* bits. */
uint32_t rsf_godot_renderer_admission(const char* driver, const char* method, uint32_t vulkan_supported);
#ifdef __cplusplus
}
#endif
