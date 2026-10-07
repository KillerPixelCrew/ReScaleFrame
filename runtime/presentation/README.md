# Presentation

This component owns the path from the application's finished source frame to one physical D3D12 presentation chain. It selects no game-specific hook sites or resource identities. The orchestrator supplies the main-window predicate, provider choice, input conventions, and graphics callbacks.

| Module | Responsibility |
| --- | --- |
| `shared_surface` | Create a typed D3D11 texture/NT handle, optionally open it on D3D12, and create a shared D3D11.4/D3D12 fence |
| `d3d11_present_bridge` | Intercept cold-start DXGI chain creation, expose an application-facing COM facade, submit source frames, and replace physical generation providers |

Public C contracts are in `include/rescaleframe/`; implementations are in `src/`. Creation errors distinguish allocation, handle export, and handle opening. Translation-runtime sharing capability is reported through those stages; successful creation alone does not establish correct rendering or generation.

## Source-frame handoff

For D3D11, the facade exposes one shared buffer and the game's D3D11 device. Before forwarding source Present, it invokes the graphics owner, signals and flushes D3D11 work, and makes the D3D12 presentation queue wait. A command list copies the shared image into the physical back buffer, returns the shared resource to COMMON, and returns the physical buffer to PRESENT. The optional `prepare` callback records additional graphics/provider work into that still-open list. D3D11 waits for upload completion before reusing its shared image; generated-frame consumers use the physical buffer.

For native D3D12, the facade uses the engine's device/queue. Runtime switching adds stable engine-facing render buffers, which are copied into the current physical provider buffer. Streamline can use its upgraded presentation queue; fence dependencies join the engine queue and presentation queue around that handoff.

Three allocator/list slots carry independent completion values. Reuse waits for the selected slot; the monotonic fence serial also counts other queue synchronization and therefore does not choose the ring slot. CPU result codes/HRESULTs report submitted operations and source Present outcomes, not scanout or latency improvements.

## Ownership and lifecycle

Install before the application's main swap chain is created. Unsupported or failed interception forwards the original creation request. Factory hooks remain for process lifetime, so callback functions and their user storage must remain available. Directory strings are copied during install.

GPU resources and both API objects of a shared surface/fence belong to their owner. Accessors return borrowed interfaces; the caller controls D3D12 resource state, monotonically increasing shared-fence values, and submitted waits/signals. Devices must use the same adapter. Retire all GPU use before destroying shared resources.

The graphics owner runs callbacks synchronously on the source Present thread. `before_present` precedes upload, `prepare` sees the open submission list, latency events bracket the physical Present, and `retire` follows the provider's `after_present`. Preserve application bindings when a callback issues D3D11 work.

Provider requests are applied at a successful source Present boundary. The bridge drains upload/presentation and provider-owned work, probes a replacement, then replaces its physical chain. Creation failure restores the old provider or falls back to plain presentation. A generation counter invalidates CPU marker identities from the retired provider.

CPU pacing/marker wrappers take a nonblocking shared provider-lifetime lock. They return NOT_READY during exclusive retirement, allowing the caller to keep window messages moving. Raw session accessors borrow the current render-owned lifetime and do not pin a retiring provider.

Resize drains work and requires application buffer references to be released. A failed physical resize recreates the prior D3D11 surface; an incomplete replacement marks the facade faulted. Destruction also drains provider work. If retirement fails, GPU-reachable storage is deliberately retained for process lifetime rather than released while still in use.

Recorded implementation and validation status are in [the tracker](../../docs/implementation.md). This documentation pass adds no new runtime measurements.
