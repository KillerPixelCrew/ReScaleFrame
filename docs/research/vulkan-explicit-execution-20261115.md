# Explicit Vulkan execution ownership

TotK now copies a verified framework calculation source through submission,
asynchronous replay, the present worker and the pending swapchain image. Source
review found that the legacy Vulkan latency service still selected Present's
identity from its latest `render_frame`. Rendering a newer source before the old
output presents would attach the old presentation to the newer token.

The shared game render interface now defines a PRESENT role, alongside the
existing SUBMISSION role. `rsf_latencyvk_execution` accepts these engine facts
with copied session/source identity and a checked structure prefix. It requires
accepted CPU ownership and a completed simulation; unknown sessions, skipped
sources, missing submissions and unpaired ends are refused. No resource pointer
or game-specific state crosses this path.

The core emits one render-start marker for the source's first actual submission.
Individual submission ends do not close it: several Vulkan submits can belong to
one frame. Present begin for that explicit source closes rendering and opens
presentation; its matching end closes presentation. A bounded per-source table
deduplicates the sequence independently of the latest renderer. Streamline token
validation still checks the SDK's six-handle lifetime. XeLL uses the same fact
sequence, and the selected AMD owner receives its Present update once. The
integration does not select vendors, sleep or map markers.

The synthetic Vulkan latency fixture starts rendering source 101 before
presenting source 100 and checks that the ending marker triplet still names 100.
It also checks duplicate submissions/presents, an unknown session, an unpaired
end and late rendering after Present. The focused fixture passes. This is
ownership/ordering evidence; it does not measure latency or establish real SDK
support. Legacy callers retain their old API until their execution sources migrate.

TotK reports the facts at actual queue calls, including recorder packets that own
copied source values, split submissions and the final blit. Present events use the
pending swapchain image's identity. Calls occur outside the Vulkan queue lock.
The low-latency setting is applied on the input boundary with the guest core
released; RSF owns provider selection and pacing. Latency-only sessions request
no DLSS SR/FG feature and initialize independently of the upscaler setting.

## Validation

The updated Vulkan latency contract fixture passes, including the overlapping
source sequence above. The full `eng/verify.ps1 -Configuration Release -VS2026`
build ran 231 tests: the same `anomaly_fg_dlss_sr5_device` and
`anomaly_fg_fsr3_sroff_device` failures prevent a green gate. Rust format and
Clippy checks were then run independently and pass. These changes do not modify
Rust behavior.

Two bounded TotK 1.4.3 opening-scene runs exercised the real RTX 4070 Laptop GPU
with driver 617.42, Vulkan validation, GPL off and descriptor writes:

- `build/checks/rsf-latency-only.log`: upscaling off, low latency on,
  asynchronous replay. Completed 1,655 presents with zero validation errors,
  shader failures and skipped draws.
- `build/checks/rsf-latency-dlss.log`: DLSS native-resolution SR and low latency
  on, synchronous replay. Completed 1,826 presents and at least 300 SR evaluations
  with zero validation errors, shader failures and skipped draws.

Both runs report Reflex owner 1, available 1, mode 1. At source 1,200 the SDK had
accepted 1,200 sleep calls and 7,193 markers with zero refused calls; CPU ownership
had zero ordering violations and input substitutes. These are accepted driver
calls and correct frame association, not a measurement of input-to-display
latency or proof of AMD/XeLL hardware behavior. The default GPL/descriptor-template
validation limitations remain open. Logs live in the TotK workspace's ignored
build directory; no game-derived artifact is committed.
