# Direct overlay service and panel admission

The source commit is `f070bd44` on
`NightHammer1000/overlay-platform-direct`, from `17274627`.

| Requested path | Source disposition |
| --- | --- |
| O02 `runtime/orchestrator/src/overlay_platform.cpp` | Removed with its private platform table. Rust implements the existing Runtime v4 exports, D3D11 observer setup, DXGI window query and D08 bound entry points. |
| D08 overlay telemetry | `overlay_service.rs` calls the Rust input-counter, input-route, FG status and options, and latency status owners directly. The D3D11 bridge still supplies its native SDK provider table. |
| D08 panel admission | `overlay_panel.rs` converts the five exports to one nonoptional table. Native D3D11 loads it directly, and its existing fake loader still supplies raw tables in unit fixtures. Native D3D12 also consumes the typed table and retains the module only at its existing successful-input-install boundary. |
| D08 native-window table | Already direct at the base. `native_window::scene_for_window` calls `native_scene::rsf_native_scene_for_window`. Scene classification also calls the Rust owner. No native-window change was required. |

The D3D11 observer still filters completed and test Presents before polling
the toggle and drawing. The native registration exception guard is in
`runtime/graphics/src/d3d11_observer.cpp`. Adapter logs use the existing
`rsf_call_native_log` guard. The observer result mapping, D3D11 host reset
fixture, D3D12 atlas fixtures and D08 bound draw closure retain their
admission and refusal handling.

D08 still passes the provided backbuffer and the separate input HWND.
The deferred status callback binds the existing FG12 platform at the same
accepted-present boundary. The C02 render and callback host methods,
saved Unicode rollback implementation, wrapped resource states and panel
wire records are unchanged.

Targeted bindgen 0.72.1 regeneration reproduced both changed bindings.
The v4 service, setup, option, diagnostics and capture records were
compared field for field with the base. The published C headers, protected
owner files and native-window source were compared with `17274627`.
Pinned Rust 1.94.1 formatting, locked offline Cargo metadata and source
whitespace checks passed. No builds, tests, lint or device execution ran.
Native linking and real-panel validation remain user-owned.
