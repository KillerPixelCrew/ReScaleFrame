# Explicit Vulkan FG presentation source

## Question and approach

Preparing frame N+1 does not establish that the next output presents N+1. The
Vulkan presenters previously selected the newest prepared slot. TotK already
carries the calculation identity with its pending output image, so the shared
runtime needs an explicit presentation source without changing the legacy API.

`rsf_fgvk_present_source` accepts the identity prefix of `rsf_frame_record`.
Its size and ABI are checked before provider dispatch. Providers select the
prepared slot by that source and require session, view and resource generation
to match. A null, zero or unmatched source presents ordinary output with FG off;
it never falls back to the newest prepared frame. The original presentation
entry point retains its existing behavior for other callers.

## Ownership and runtime use

The call borrows the identity only for the duration of Present. It does not
retain game images or establish a submission lease. Prepared depth/motion and
HUD-less snapshots still need their existing successful submission notifications
and provider completion tracking. Source selection does not bypass those checks.
The FSR 3 and DLSS owners preserve the producer's newest frame when presenting
an earlier source. An unrecognized output invalidates outstanding presentation
eligibility conservatively rather than reusing old input after a gap.

TotK `runtime/gfx_present.c` `flip_locked` supplies the identity attached to the
pending swapchain image, under its presentation lock. Direct/unknown outputs
pass null. The current adapter session and view are both 1; owner recreation
retires its prepared resources. Interpolation remains disabled while HUD-less
capture and scene submission notification are being connected.

## Evidence and limits

The native FSR 3 submission and DLSS owner fixtures prepare N and N+1 before
presenting either. Explicit N selects N, leaves the producer at N+1, and permits
N+1 on its own Present. They pass on Windows Release. The FSR 3 fixture also
checks short identities, wrong ABI, foreign session, NO_FG and unknown sources.
All four dispatcher variants and the runtime DLL export check pass. TotK's
provider lifetime fixture passes with the identity forwarded intact.

These are synthetic ownership and routing checks, not generated-image quality
acceptance. The XeSS and FSR 4 bridge paths still use numeric source IDs for
parts of their presentation progression; restart/wrap and unknown-output gaps
need separate device acceptance before those integrations are considered
complete. No performance or latency improvement is inferred from these tests.

TotK's bounded opening-scene runs using this runtime reached the world and
exited normally: FSR 3 WSI completed 1,578 presents and DLSS WSI completed 1,581,
each with zero Vulkan validation errors, shader failures or skipped draws.
Interpolation was off, SR off and low latency on, using GPL off and descriptor
writes on the RTX 4070 Laptop GPU. The Vulkan execution probe also passes.
The full Release gate ran 232 tests and retains the previously recorded
`anomaly_fg_dlss_sr5_device` and `anomaly_fg_fsr3_sroff_device` failures. Rust
formatting and Clippy pass independently. Logs are in TotK's ignored
`build/checks/rsf-fg-source-*` files.
