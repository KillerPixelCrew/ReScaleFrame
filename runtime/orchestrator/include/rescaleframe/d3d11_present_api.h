/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_D3D11_PRESENT_API_H
#define RSF_D3D11_PRESENT_API_H

#include <rescaleframe/runtime.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/d3d11_present_bridge.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_D3D11_PRESENT_API_VERSION 1u
#define RSF_D3D11_PRESENT_API_ENTRY_POINT "rsf_get_d3d11_present_api"

/* DLL access to the existing process-wide presentation bridge, not a new owner.
   Function signatures and results are those in d3d11_present_bridge.h: install,
   prepare, graphics and owner predicates return int booleans; create_for_hwnd
   returns an HRESULT; other int32_t operations return RSF_BACKEND_* results.
   A populated table proves ABI availability only, not device/provider readiness.

   Query is reentrant, thread-safe and graphics-free. The caller owns the table
   and may copy it. Pointers remain valid only while this Runtime DLL is loaded.
   Do not statically link another OrchestratorCore or presentation bridge into
   the adapter. Querying does not install hooks, prepare settings or load SDKs.

   Execution and ownership (the bridge does not add caller synchronization):
   - install OR prepare: serialized cold startup, before the main swapchain.
     install adds process-lifetime DXGI hooks; prepare plus create_for_hwnd is
     the adapter-owned creation route without those creation detours. Do not
     call from DllMain or concurrently with graphics/another setup call.
   - Setup and strings are borrowed during setup; the bridge copies settings
     and path/identity strings. Callback code and user data must remain valid
     for every later callback, including retirement. No uninstall is exposed:
     keep both Runtime and callback-owning DLLs loaded for process lifetime.
   - create_for_hwnd runs at the serialized engine creation site. Parameters
     are borrowed; description is a DXGI_SWAP_CHAIN_DESC1. On success out owns
     one IDXGISwapChain1 COM reference, which the caller must Release.
   - graphics: D3D11 execution thread. Initialize graphics.struct_size. Returned
     device/SR-upload queue aliases are borrowed; native_queue is null. They
     are not the physical FG provider's presentation queue.
   - host/provider/session/queue/command_list: use on the Present owner thread,
     or in its serialized prepare/retire callback, without racing a transition
     or chain destruction. These are borrowed, not independent leases. Never
     destroy the host or provider session. Queue and provider/session snapshots
     must not cross a drained provider transition; host/device objects must
     not outlive their owning bridge. command_list's alias is borrowed for the
     input list's lifetime. Independent getters are not an atomic snapshot.
   - retire: retire callback only. A supplied list is closed and created on the
     shared device (or null). The returned borrowed fence/value cover vendor
     reads, the list and earlier provider-queue work on success; AddRef the fence
     to keep it. Failure clears both outputs and signals nothing.
   - is_owner/has_owner/backend/backend_choices/switch_result/generation are
     observations, not lifetime leases. request is a control request, applied
     by the Present owner at a drained boundary, not an immediate switch.
   - begin/marker/acquire/input/abort are main-thread CPU events. Carry one real
     frame ID through CPU, submission and Present; use begin's generation for
     marker. A stale generation is refused. During retirement these calls may
     return NOT_READY rather than block window messages. Do not call begin in
     addition to another pacing owner for the same frame.

   This table does not add a frame.ui transport to native_fg. Native FG's missing
   frame.ui input remains an integration gap; exposing UI modes is not proof
   that a separate UI layer can be submitted by that path. */
typedef struct rsf_d3d11_present_api {
    uint32_t struct_size;
    uint32_t abi_version;
    int (*install)(const rsf_d3d11_present_setup* setup);
    int (*prepare)(const rsf_d3d11_present_setup* setup);
    int32_t (*create_for_hwnd)(void* factory, void* queue, void* hwnd,
                               const void* description, void** out);
    rsf_streamline_host* (*host)(void);
    int (*graphics)(rsf_streamline_graphics* graphics);
    void* (*queue)(void);
    void* (*command_list)(void* list);
    int32_t (*retire)(void* provider_session, void* list, void** fence, uint64_t* value);
    int (*is_owner)(void* swapchain);
    int (*has_owner)(void);
    const rsf_generation_provider* (*provider)(void);
    void* (*session)(void);
    uint32_t (*backend)(void);
    uint32_t (*backend_choices)(void);
    int32_t (*request)(uint32_t backend);
    int32_t (*switch_result)(void);
    uint64_t (*generation)(void);
    int32_t (*begin)(uint64_t id, uint64_t* generation);
    int32_t (*marker)(uint64_t generation, uint64_t id, rsf_latency_marker marker,
                      uint32_t controller);
    int32_t (*acquire)(uint64_t id);
    int32_t (*input)(uint64_t id, uint32_t kinds, uint32_t message);
    int32_t (*abort)(uint64_t id);
} rsf_d3d11_present_api;

/* Initialize struct_size to writable capacity and abi_version to VERSION above.
   Null or capacity < sizeof(rsf_d3d11_present_api): INVALID_ARGUMENT, without
   reading abi_version. Otherwise a version mismatch: ABI_MISMATCH. Size refusal
   takes precedence over version refusal. Failure never writes any caller bytes
   or changes bridge state. On OK the known prefix is filled, struct_size is the
   runtime's sizeof(table), and bytes beyond it are untouched. Storage must be
   suitably aligned and contain at least the readable struct_size field; claimed
   full capacity must actually be writable. Append fields and bump VERSION for
   incompatible revisions; this query does not negotiate older versions. */
RSF_RUNTIME_API rsf_result rsf_get_d3d11_present_api(rsf_d3d11_present_api* api);
typedef rsf_result (*rsf_get_d3d11_present_api_fn)(rsf_d3d11_present_api* api);

#ifdef __cplusplus
}
#endif
#endif
