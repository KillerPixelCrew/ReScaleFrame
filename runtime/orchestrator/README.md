# Orchestrator

The reusable runtime owns backend sessions, selection, history continuity and graphics interop.
Game plugins own detection, hooks, renderer preparation, input conventions and reinsertion.
The SR APIs accept no game identifier, signature or engine address.

| Module | Purpose |
| --- | --- |
| `sr_session` | C ABI over prepared D3D12 frames; transactional FSR2/FSR3/FSR4/XeSS selection and status |
| `sr_bridge` | Same-adapter D3D11/D3D12 copies, shared fences, resource states and synchronized SR output |
| `sr_legacy_adapter` | Private conversion from the existing pipeline into SDK records and prepared inputs |
| `dlss_pipeline` | Compatibility entry points for the working D3D11 path, including switching to the new backends |
| `frame_assembly` | Existing decoded-camera/resource validation; legacy camera type has a distinct name |
| `runtime` | DLL version entry point |

Direct sessions consume dense motion in previous-minus-current render pixels, device depth,
pre-tonemap scene color, jitter, exposure convention and matching SDK frame/view records. The
caller owns the resources and graphics thread. It must finish submitted D3D12 work before switching
or destroying a direct session. The bridge performs that synchronization for its D3D11 caller.

A switch opens and plans the replacement before releasing the active backend. Failures retain the
old backend and are reported separately from evaluation failures. The next accepted frame resets
history after selection, gaps, refusals, view/session changes and resource-generation changes.
Separate paths and optional provider IDs let FSR SDK releases coexist; requested family and actual
provider version must agree. Native XeSS D3D11, FG and latency are not implemented here.

The Insert overlay switches the compatibility pipeline on its render thread. AC7 still starts DLSS
first. The carrier supplies its engine identity, world-unit scale, game hooks and output reinsertion.
The newer direct APIs can start on FSR or XeSS independently of DLSS. Moving plugin preparation,
tap ownership and settings into the public DLL/plugin lifecycle remains separate work.

The synchronous bridge has serialization overhead. FSR2/FSR3/XeSS have synthetic device evidence
on Intel UHD and RTX 4070 Laptop; the compatibility switch sequence also passed on the RTX.
FSR4 refusal is tested, but its evaluation requires compatible hardware. New AC7 backend image
quality and transition tests are pending. See [research and checks](../../docs/research/orchestrator-sr-switching.md).
