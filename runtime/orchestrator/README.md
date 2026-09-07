# Orchestrator

The orchestrator owns shared frame processing and vendor backends. The eventual plugin/settings lifecycle is still scaffolding; the research proxy currently drives the working DLSS pipeline.

| Module | Purpose |
| --- | --- |
| `runtime` | Version entry point |
| `frame_assembly` | Validate camera/resource inputs and build a DLSS frame |
| `dlss_pipeline` | Manage output/decode resources, backend setup, rebuilds, and evaluation |

Frame assembly checks the camera data, jitter, resource sizes, and decoded-motion requirements. It accepts native-size output or a larger output. Missing inputs return a refusal rather than being guessed.

The caller supplies the D3D11 device/context and a matching camera/resource set. Resource changes rebuild the pipeline and reset history. Engine camera cuts, missed-frame continuity, render-thread commands, and full teardown still need work.

The live AC7 bridge evaluates at the composite gate when reinsertion (F6) is on and at Present otherwise. The [representation plan](../../docs/representation-plan.md) turns this scaffold into the session that owns the tap, the pipeline, the UI layer and the presentation bridge, renames `dlss_pipeline` to a vendor-neutral `sr_pipeline`, and stops `frame_assembly.h` including the DLSS header. See [design](../../docs/design.md), [todo](../../docs/implementation.md), and [review](../../docs/review.md).
