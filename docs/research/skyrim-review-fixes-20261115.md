# Skyrim bounded review fixes, 15 November 2026

Question: correct verified lifecycle and optional-input failures from
`.local/skyrim-research/opus-review-20261115.md` without replacing native
history, shader production or runtime vendor policy. This is source review
against the exact Skyrim1.7.104 path and existing audit, not a new native
experiment. Fingerprint and parity run scope are retained in
[the parity note](skyrim-cs-parity-input-wiring-20261115.md).

- A1: only the GetProcAddress call that transitions armed to captured substitutes
  the CS initializer wrapper. Repeated lookups return the actual native export,
  preserving LastError and avoiding a second wrapper CAS that refused CS startup.
- A2: missing target85, wrong device/extent/mips/slices/samples or unsupported
  numeric format leaves temporal_mask null and weights zero, preserving SR.
  Colour/depth/motion validation remains mandatory. Allocation AddRef is still
  a synchronous lease, not proof that contents survive later native writes.
- A5: plugin rollback and potentially BUSY stop use the CPU owner's cpu_disable
  (quiesce/disable/drain, retaining prepared host services). cpu_stop releases
  those services only after all other stop checks pass. A retry therefore keeps
  the prepared session identity. CPU implementation belongs to its separate owner.
- A6: the jitter callback always configures resolution, passing an empty host
  after compatibility refusal. The existing disabled path restores saved ratios
  and lock on the native render owner. An active auxiliary handoff finishes first;
  its existing output_active guard defers restoration to a later jitter callback.
- A7: SR reasserts NO_FG after CPU identity classification; scalar presentation
  fallback also refuses FG unless its producer was FG_INPUTS. The existing
  same-session/frame/view/source/submission/scope merge retains an earlier real
  FG_INPUTS identity when the later SR callback arrives, so a good pre-mutation
  input lease is not replaced by SR-only metadata. Later CPU refusals still win.
- A10/A12: presentation window/chain/context/frame/UI singleton reads are guarded;
  resolution's heap window/chain/context reads use checked ReadProcessMemory.
  observe_end copies services under lifecycle before native execution and uses
  the copy for presentation binding and pending auxiliary completion.
- A13: start rechecks the existing matched native guard set while hooks are
  disabled, before enabling the plugin hooks. Presentation/sampling starts also
  recheck their guards on disabled retries. Already-live hooks skip the original
  byte comparison because their owned MinHook jump is expected. These checks
  narrow prepare-to-start mutation exposure; they are not an atomic guarantee
  against an unrelated patch racing immediately after a check.

B3: the plugin capability comment now describes prospective-poll ownership and
runtime policy. The top implementation tracker and engine evidence distinguish
the active INPUT_TIMELINE connection from historical rendered-source runs; CPU
header/log terminology is corrected by its owner. The older prospective-source
note's statement that the capability stayed disabled describes its earlier
verification batch, superseded by the bounded acceptance in the parity note.
B4: the weights are not inferred from the unread native mask. Inspected CS
`038a0a04ce9db5b84707ec662ddfe7824cdac3dd` Upscaling.cpp selects
kTEMPORAL_AA_MASK and EncodeTexturesCS.hlsl computes X*0.1+Y for the bias hint.
The adapter supplies that scalar recipe; saturation and maximum composition
are the generic runtime contract. Captured normalized values are evidence of
execution, not proof of native X/Y equality until the raw readback succeeds.

Other correctness dispositions are owned and explained in
[CPU review fixes](skyrim-cpu-review-fixes-20261115.md) (A3/A4/A5/A9/A11/A13)
and [prospective refusal](prospective-source-refusal-20261115.md) (A8/B1).
A9 is latent on the inspected unconditional RenderEntry path and was not
repaired by fabricating simulation end. B2 remains an explicit acceptance gap:
bounded DLSS/Reflex evidence is not XeSS FG or broad low-latency acceptance.

The sampler simplification moves scene eligibility and pending-restoration
early-out ahead of the16-slot native sampler scan. Pending restoration still
scans and restores on the original render thread; active scene binding still
validates every sampler each time. No dirty-mask cache or snapshot framework
was introduced. The unused resolution cmath include was removed.

Remaining C/D/E and S-alt suggestions are optional architecture or unmeasured
efficiency proposals, not proof of a defect requiring a broad rewrite. Shared
snapshot builders, hook frameworks, dirty-mask caches, per-frame provider fallback
and a new acknowledgement ABI were deliberately not introduced. Native engine
facts and vendor policy remain on their existing sides of the C ABI boundary.
Known coexistence limits remain: CS's exact-profile pre-initializer exclusion
cannot establish ownership when CS initialized first; CS HDR stays refused
pending applied colour-space and post-composite scene ownership, despite SR
float16 format admission. ENB and SSEDisplayTweaks interaction awaits the
coordinator's authorized compatibility runs; RSF has no new SKSE dependency.

Initial worker validation was source/diff review and git diff --check only.
The coordinator subsequently ran `eng/verify.ps1 -Configuration Release -VS2026`:
`.local/skyrim-research/verify-skyrim-opus-fixes-20261115.log` records exit0,
220 passed/14 skipped, with Cargo fmt/Clippy clean. The concrete repair batch
is built and gate-tested; optional architecture proposals remain intentionally out.

The coordinator's PID5484 bounded run
(`.local/skyrim-research/parity-acceptance-20261115-234222`) completed F10,
exited0 and hash-verified restoration of Runtime/Entry/INI/carrier. Native
R8G8_UNORM target85 cropped to1280x720 matches both normalized R32 masks
exactly under clamp(0.1R+G); source/backend colour match with transport off.
Physical client1920x1080 matches output at144DPI. The Survival Mode modal
remained: SceneNotEligible and no FG tags establish safe refusal under that
modal, not gameplay FG acceptance. Full run identities and numeric results are
in [the parity note](skyrim-cs-parity-input-wiring-20261115.md).

These observations cover the bounded resource path, not every rollback,
repeated initializer lookup or foreign-hook mutation branch. Attributable
pickup, XeSS FG, broad timing/menu/pause coverage and the newly requested
major-mod coexistence runs remain outstanding. No build/game/deploy/commit
was performed by this documentation worker; the coordinator owns those actions.
