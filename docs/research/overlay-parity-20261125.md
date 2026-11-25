# Overlay parity across games, 25 November 2026

Every game offers the same overlay controls and values. A value the machine cannot serve is drawn
greyed with its reason. It is never absent. Built and unit-tested, not game-run; no device or
visible-window fixture was started.

## The common set

| Control | Values in every game | Greyed when (reason shown by the panel) |
| --- | --- | --- |
| SR backend | Auto, DLSS, FSR2, FSR3, FSR4, XeSS, FSR1 (the engine's own scaler) | DLSS: not an NVIDIA adapter or no Streamline files. FSR2/3/4: no FidelityFX upscaler files. XeSS: no `libxess.dll`. Auto: nothing else is offered. FSR1: the engine has no scaler |
| SR quality | Native, Quality, Balanced, Performance, Ultra perf, Ultra Quality | Ultra Quality unless the backend is XeSS or FSR1; all of them while the engine owns input resolution |
| DLSS model | Default, E, F, J, K, L, M | DLSS is not offered |
| FG provider | Off, Auto, DLSS-G, FSR3, FSR4, XeSS | No provider files or GPU (FSR4 FG: not RDNA 4) |
| Multiplier | 2x up to the provider's reported maximum | Above the provider's maximum |
| Latency | Off, On, Boost (Reflex), On/Off for XeLL and Anti-Lag | The GPU reports no low-latency technology |
| Frame limit | steppers and presets | No low-latency technology (see follow-up 9) |
| Sharpening | Off, RCAS, Depth RCAS, Depth DAS, Vendor, and their options | No upscaler running, or the engine's scaler selected |
| Status overlay, panel size and side | the same | never |

The single source is the Runtime:

- `runtime/orchestrator/src/sr_choices.rs`: `sr_backends(vendor, has-package, engine_spatial)` and
  `sr_quality(backend)`. FSR2, FSR3 and FSR4 share one FidelityFX package, so one file check serves all
  three. Every SR host reads these: `d3d11_host.rs` (`sr_offer`), `d3d11_sr_admission.rs` (Anomaly),
  `render_host.rs` (RDR2), `callback_host.rs` (Unity), `dlss_pipeline.rs` (AC7, through the new
  `rsf_dlss_pipeline_backend_choices`).
- `fg_choice.rs::resolve`: FG Auto per GPU among the providers the bridge serves. `overlay_service.rs`,
  `callback_host.rs`, `render_host.rs` and AC7 (`rsf_overlay_request_fg_backend`) use it.
- `RSF_SR_CHOICES_ALL`, `RSF_FG_CHOICES_ALL` and `RSF_OVERLAY_MAX_GENERATED_FRAMES` in the contract headers
  replace the per-adapter lists. An adapter passes them when it has no limit of its own.

## Audit before the change

| Game | SR mask (file:line) | SR quality | FG mask | Latency, max generated | Notes |
| --- | --- | --- | --- | --- | --- |
| AC7 | `loader/proxy/src/dlss_bridge.c:335` DLSS, FSR2, FSR3, FSR4, XeSS, FSR1; no Auto | standard (`:493`) | `overlay_settings.rs:65` bridge choices, no Auto | vendor-reported | Auto SR and FG missing: (c) |
| Anomaly | `overlay.rs:312` from `rsf_d3d11_sr_admit`, adapter limit `sr_stage_platform.h:136` 0x3a plus Auto = 0xba; no FSR2 | 0x1f | `overlay.rs:337` all plus Auto, Auto then stripped at `overlay_service.rs:1214`; zero choices unless the bridge is installed (`:342`) | modes `:338`, max 3 (`:340`) | FSR2 missing and max 3: (c). FG off by default: (a), follow-up 4 |
| Skyrim SE, Fallout 4 | `d3d11_host.rs:52` 0x3e: no Auto | adapter: Skyrim 0x1f, Fallout 4 Native only without `rsf_d3d11_host_reduced_frame` (`games/fallout4/src/session.rs:148`) | `d3d11_host.rs:699` Auto, DLSS-G, FSR3, FSR4, XeSS, Auto stripped at `overlay_service.rs:1214` | full with the input timeline (`:761`), max 15 | Auto missing: (c). Fallout 4 quality: (a) input, follow-up 2 |
| RDR2 | `games/rdr2/src/runtime_worker.rs:194` 0x3a: no FSR2, Auto, FSR1 | standard (`render_host.rs:1171`) | `render_host.rs:1206` bridge choices plus Auto | vendor-reported | FSR2 and SR Auto missing: (c). FSR1: (a), follow-up 1 |
| Unity Mono | `games/unity-mono/src/runtime_setup.rs:59` 0x7e: no Auto although the saved default is Auto | `callback_host.rs:150`: extended for every backend but DLSS | `overlay_settings.rs:65` without Auto | vendor-reported | SR and FG Auto missing: (c). Ultra Quality offered on FSR3 although `d3d12_sr/fsr.rs:237` refuses it |
| Project Wingman | no overlay host | none | none | none | detection scaffold only: (a), follow-up 5 |

The user's two data points, checked against the code: Anomaly was the only game offering SR Auto (the admission
adds the bit, `d3d11_sr_admission.rs`): its adapter limit 0x3a plus Auto is the 0xba the Rust sessions log. It did
not offer FSR2 (bit 2 is absent from 0x3a); every other D3D11 host, Unity and AC7 did. FG Auto reached the panel
only in RDR2.

## Per game after the change

Works now means it was already served. Wired means this change connects it.

| Game | SR Auto | FSR2 | FSR1 | FG Auto | Everything else in the common set |
| --- | --- | --- | --- | --- | --- |
| AC7 | wired: `dlss_pipeline.rs` `select_backend` resolves it | works (pipeline serves 2..5) | blocked on follow-up 1 | wired: `rsf_overlay_request_fg_backend` | works now; the mask is the Runtime's (unverified in C, see below) |
| Anomaly | works (admission) | wired: adapter limit 0x3e | blocked on follow-up 1 | wired: `overlay_service.rs` `forward_fg` | max generated now 15; FG controls greyed until the bridge is installed (follow-up 4) |
| Skyrim SE | wired: `d3d11_host.rs` `concrete_backend` | wired: package file check | blocked on follow-up 1 | wired: `forward_fg` | quality levels by the engine's reduction; Ultra Quality blocked on follow-up 3 |
| Fallout 4 | wired as Skyrim | wired | blocked on follow-up 1 | wired | quality blocked on follow-up 2 |
| RDR2 | wired: panel shows Auto selected, resolves as before | wired: mask only, `sr12` serves 2 | blocked on follow-up 1 | works (already offered) | Ultra Quality offered for XeSS |
| Unity Mono | wired: `apply_intent` accepts Auto | works | works (URP FSR1) | wired: `callback_host.rs` | Ultra Quality only for XeSS and FSR1 |
| Project Wingman | blocked on follow-up 5 | blocked on 5 | blocked on 5 | blocked on 5 | blocked on 5 |

Hardware or package impossibility, decided at run time by the Runtime: DLSS and DLSS-G on a non-NVIDIA adapter,
FSR4 FG off RDNA 4 (`rsf_generation_fsr4_supported`, already present), any provider whose files are missing.

## Follow-ups that could not be finished here

1. Done for D3D11 on `NightHammer1000/wave-d3d11` (b256fa81, the shared `sr12/easu.hlsl`) and for D3D12 on `NightHammer1000/wave-d3d12` (81c2319c). Original note: Runtime-owned FSR1 for every D3D11 and D3D12 host. The engine scaler exists only in Unity (URP) and Godot.
   Design: a spatial provider (EASU then the shared RCAS) beside the FSR/XeSS providers. It needs colour and
   output size only, no depth, motion or jitter, so every host already supplies its input. Anchors:
   `sr_session.rs:235` and `dlss_pipeline.rs` `select_backend` accept ids 1..5 only; `sr_bridge`/`d3d12_sr/*` need
   the compute pass; `sr12.rs` and `d3d12_generation/*` carry the plumbing and were off limits. Then pass
   `engine_spatial: true` from `sr_backends` for hosts with the provider.
2. Fallout 4 reduced-resolution quality. `games/fallout4/src/session.rs:148` offers Native only unless the
   runtime exports `rsf_d3d11_host_reduced_frame`. The missing input is the engine's render-scale hook, which
   belongs in `games/fallout4` (Rule 5).
3. Ultra Quality with XeSS on D3D11 hosts. `d3d11_host.rs` `validate_setup` rejects quality bits above 0x1f, and
   `overlay_service.rs` keeps one static quality mask per host. Make the service intersect the mask with
   `sr_quality(backend)` and raise the bound; check the `dlss_pipeline` quality range.
4. Done on `NightHammer1000/wave-d3d11`: Anomaly installs the presentation bridge whenever its FG stage runs (Mode=Observe); the `FGBridge` switch and its panel option are gone, so FG can be switched on in the overlay in a default session as in every other game.
5. Project Wingman has no render host or overlay (`games/project-wingman`): an adapter that supplies colour,
   depth, motion, jitter and exposure to the D3D11 or render host is the work; the panel then needs nothing.
6. Rendered-source FG (`d3d11_host.rs` `fg_choices(true, true)`) offers FSR3 and FSR4 only, because DLSS-G and
   XeSS FG need the input timeline. No shipped adapter declares rendered sources.
7. DLSS is offered by vendor id and files. A non-RTX NVIDIA GPU still shows DLSS selectable until Streamline
   refuses it at start. A pre-start RTX query is needed. FSR2 and FSR4 likewise depend on the provider versions the
   DLL exposes (`ffx_version::admitted`), known only at start.
8. AC7 shows the resolved backend after choosing Auto, because `pipeline.requested_backend` is concrete. The panel
   marks that backend, not Auto.
9. The frame limit is greyed whenever no low-latency technology is reported (`ui/overlay/src/panel.rs`
   `frame_limit`). The limit is pacing, not Reflex; decoupling it belongs in `frame_limit.rs`.
10. Godot's `gl_pipeline/settings.rs:50` keeps its own SR list. It was outside the six games.

`loader/proxy/src/dlss_bridge.c` changed (SR mask, FG request) and was not compiled: no CMake build was allowed.
The Rust exports it calls are checked by `cargo check` and unit tests.

## Behaviour changes to know

- The SR panel shows greyed controls instead of an empty card when no provider is available.
- Unity no longer offers Ultra Quality on FSR2, FSR3 and FSR4 (the provider refuses it).
- A host with no package files reports no SR, where it used to offer DLSS and fail at start.
- `RSF_FG_BACKEND_AUTO` is offered in the D3D11 service hosts; it resolves per GPU once the first Present names
  the adapter, and keeps the bridge's own start before that.

## Checks

`cargo test -p rescaleframe-overlay` (64), `-p rsf-orchestrator --lib` (398), `-p rsf-anomaly --lib` (171),
`-p rsf-rdr2`, `-p rsf-unity-mono`, `cargo check --workspace --tests`.
