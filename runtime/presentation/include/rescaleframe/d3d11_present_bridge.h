/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_D3D11_PRESENT_BRIDGE_H
#define RSF_D3D11_PRESENT_BRIDGE_H
/* Cold-start DXGI facade over one physical D3D12 presentation chain. D3D11 renders into a shared
   surface; native D3D12 uses the engine queue, with stable facade buffers when runtime switching
   is enabled. The bridge owns transfers, fences, and provider replacement. Graphics callbacks run
   synchronously on the source Present thread. Setup/callback user storage must outlive hooks. */
#include <rescaleframe/streamline_host.h>
#include <rescaleframe/d3d11_observer.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Size-checked setup. Directory strings are copied by install; callback/user pointers are kept. */
typedef struct rsf_d3d11_present_setup {
    uint32_t struct_size;
    const char* runtime_directory_utf8;
    uint32_t development_runtime;
    rsf_backend_log_fn log;
    void* user;
    /* Window-selection policy during creation; hwnd is borrowed. Nonzero allows interception. */
    int (*accept_window)(void* user, void* hwnd);
    /* Before source-frame upload, with the application-facing chain. Preserve context state. */
    rsf_observer_present_fn before_present;
    /* Source Present start/completion; excludes generated physical presents. */
    rsf_observer_present_event_fn present_event;
    /* Brackets the physical Present after upload/prepare, when submission reaches that point. */
    rsf_observer_present_event_fn latency_event;
    /* After provider after_present; lets the graphics owner record retirement of its inputs. */
    void (*retire)(void* user, void* provider_session);
    /* Graphics owner, after upload to the physical backbuffer, before submission/Present.
       The list is open. All pointers are borrowed for this callback. */
    void (*prepare)(void* user, void* chain11, void* context11, void* list12, void* backbuffer12,
                    rsf_streamline_host* host, void* provider_session, uint32_t sync_interval);
    uint32_t debug_timing;
    /* Cold-start generation owner, independent of SR. Zero selects Off with runtime switching
       enabled, otherwise the DLSS default.
       Also accepts native D3D12 queues; no D3D11 facade or upload is used on that path. */
    uint32_t backend, max_generated_frames;
    /* Positive units_to_meters supplies explicit plugin depth conventions. Zero keeps the
       legacy D3D11 defaults; native D3D12 defaults to reversed, finite depth in meters. */
    uint32_t depth_inverted, depth_infinite;
    float units_to_meters;
    /* Native D3D12 keeps engine buffers stable while swapping the physical presentation
       provider at a drained Present boundary. Off retains the facade and plain presentation. */
    uint32_t runtime_switching, ui_mode;
    const char* fsr3_directory_utf8;
    const char* fsr4_directory_utf8;
    const char* xess_directory_utf8;
    const char* streamline_directory_utf8;
} rsf_d3d11_present_setup;
/* Cold startup only, before the game's main swapchain. Unsupported creation forwards unchanged.
   No game-specific sites or policies live in the bridge. Hooks remain for process lifetime. */
int rsf_d3d11_present_install(const rsf_d3d11_present_setup* setup);
/* Borrowed process-owned host, or null before activation/when that provider path has no host. */
rsf_streamline_host* rsf_d3d11_present_host(void);
/* Source render thread: borrow the active facade's graphics device and interop queue. D3D11 uses
   a dedicated SR queue distinct from physical Present; native D3D12 returns the engine queue.
   native_queue is null in the returned table. No references are acquired; preserve facade lifetime. */
int rsf_d3d11_present_graphics(rsf_streamline_graphics* graphics);
/* Compare facade identity; these checks acquire no COM reference. */
int rsf_d3d11_present_is_owner(void* swapchain);
int rsf_d3d11_present_has_owner(void);
/* Static API table for the active backend; null when backend Off is effective. Session is
   borrowed and may retire at a successful Present switch or facade destruction. Use the render boundary or wrappers
   below to keep its lifetime stable while invoking it. */
const rsf_generation_provider* rsf_d3d11_present_provider(void);
void* rsf_d3d11_present_session(void);
uint32_t rsf_d3d11_present_backend(void);
/* Queue a backend (zero means Off) for the next successful source Present boundary. Last request
   wins. Returns backend OK/NOT_READY/NOT_SUPPORTED; acceptance is not switch completion. */
int32_t rsf_d3d11_present_request(uint32_t backend);
/* Most recent switch result. Failure may leave the old backend or plain presentation active. */
int32_t rsf_d3d11_present_switch_result(void);
/* Increments when the physical provider changes, including fallback after a failed request. */
uint64_t rsf_d3d11_present_generation(void);
/* CPU pacing/latency wrappers use a nonblocking shared lifetime lock. NOT_READY means a switch
   owns retirement. begin optionally returns its generation; marker rejects a stale generation.
   Frame id and marker order are supplied by the caller's lifecycle. An absent provider/host is
   generally a successful no-op; these calls do not prove generation or latency is active. */
int32_t rsf_d3d11_present_begin(uint64_t id, uint64_t* generation);
int32_t rsf_d3d11_present_marker(uint64_t generation, uint64_t id, rsf_latency_marker marker, uint32_t controller);
int32_t rsf_d3d11_present_acquire(uint64_t id);
int32_t rsf_d3d11_present_input(uint64_t id, uint32_t kinds, uint32_t message);
int32_t rsf_d3d11_present_abort(uint64_t id);
#ifdef __cplusplus
}
#endif
#endif
