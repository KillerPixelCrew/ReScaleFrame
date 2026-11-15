# Skyrim CPU review fixes, 2026-11-15

Scope: source fixes for A3/A4/A5/A11/A13 in the local Opus review
(`.local/skyrim-research/opus-review-20261115.md`), with C1/C2 cleanup.
This is source inspection, not a new binary experiment or runtime acceptance.
Existing working-tree timeline edits were retained. No readiness flag was changed.

The question was whether activation, quiescence and retry could leave the CPU
observer permanently busy or without valid input provenance or host services.
The bounded approach changes only the existing hook lifetime and scalar ownership
mechanisms, preserving the native calls and their order.

- A3: tick checks active/current before taking Entry, then rechecks active under
  Entry before reading services. Inactive pass-through uses the process-lifetime
  trampoline. A tick already admitted remains leased until its native call returns.
- A4: an unowned mid-tick submit with no live ticket no longer poisons ownership.
  A later verified owned submit can recover state 3 only after observing native
  requested=running=0. A live or publishing ticket is never replaced by recovery;
  overlap still invalidates provenance. Scalar tests cover recovery and refusal
  of a live ticket even when the caller asserts idle. The production idle check
  remains at caller RVA 0x658b14 with the matching Main owner.
- A5: cpu_disable quiesces/disables and checks outstanding entries, retaining
  services on success and failure. cpu_stop releases services only after that
  succeeds. Adapter worker ctx_87a8dd3e4a5f accepted the contract: rollback and
  potentially BUSY stop paths use cpu_disable; final successful stop uses cpu_stop.
- A11: after the synchronous RESERVE callback, observe whether the scalar source
  is still reserved. If withdrawal already won, repeat ABORT after RESERVE and
  suppress subsequent sampling for that source. Serialized native polling makes
  this a defensive observation, not a claim of reproduced concurrency.
- A13: immediately before enabling any CPU hooks, require all owned hooks disabled
  and created, recheck prologues, screen-query guards and the named callback.
  A failed rollback with an enabled owned hook refuses start without comparing
  our own MinHook jump to the original bytes. This narrows, but cannot eliminate,
  a concurrent foreign patch race after validation.

SIMULATION_BEGIN remains an engine fact on the verified normal-simulation submit,
including unbound/refused input. The shared-core worker was informed to handle
refused-BIND marker admission separately. A9 adds no synthetic SIMULATION_END:
the review identifies RenderEntry RVA 0x659319 as unconditional after submit on
1.7.104; the observed RenderEntry remains the source of that event. Frame closure
continues to report RENDER_SKIPPED when RenderEntry was not observed.

C1 removes unused writes to the copied reservation phase; ProspectiveSources owns
its phase. C2 diagnostics read the immutable adoption facts, using the released
owned phase for bound status. Stale CPU header/log wording was updated for the
activated input timeline. Other C/E suggestions were left out to keep this fix
bounded; in particular no player singleton cache or actor hook lifetime change.

Checks: scoped git diff --check passed; source review confirmed all original native
calls remain, idle recovery is used only after the existing native idle check,
and disable retains services until final release. Added scalar assertions are
not executed by this worker. No build, test executable, game/GPU run, deployment
or commit was performed; coordinator owns validation and tracker consolidation.
