# Repository skills update, 2 October 2026

The user requested UE4.18 and TrueSky skills, an overhaul of existing repository skills from
the session transcript, and a separate skill for general Unreal 4 knowledge. This fork handles
skills and documentation; another session handles the startup hint and release package.

The question is how to retain the working plugin's producer/lifetime model without repeating
earlier fixes or treating AC7 addresses as universal engine facts. Skills use the shared Agent
Skills format under `.agents/skills`, with lightweight client metadata and linked references.
No personal instructions, runtime hooks or live game settings are changed by this documentation work.

## Routing and evidence

| Skill | Use it for | Detailed evidence |
| --- | --- | --- |
| [game-render-analysis](../../.agents/skills/game-render-analysis/SKILL.md) | Build/source matching, producer analysis, capture choice, hypotheses and acceptance | [Session lessons](../../.agents/skills/game-render-analysis/references/session-lessons.md) |
| [unreal4-render-integration](../../.agents/skills/unreal4-render-integration/SKILL.md) | Shared UE4 view/graph/UI/RHI ownership without fixed private offsets | [Methodology](methodology.md) and linked version-specific evidence |
| [ue418-render-integration](../../.agents/skills/ue418-render-integration/SKILL.md) | Matched 4.18.3 source and AC7 integration | [Native ownership](../../.agents/skills/ue418-render-integration/references/ac7-native-ownership.md), [hook map](ue418-hook-map.md) |
| [truesky-render-integration](../../.agents/skills/truesky-render-integration/SKILL.md) | Cloud projection, resolution/permutation, raw depth precision, native views/history | [AC7 depth](../../.agents/skills/truesky-render-integration/references/ac7-depth.md), [aircraft/cloud research](ac7-plane-artifacts-20261002.md) |

There was one existing repository skill, `game-render-analysis`. It is rewritten around native
producers and routes to the three additions. All four remain normally discoverable; each
capability has one shared source rather than copied client-specific instructions.

## Discovery and rationale

The parent 30 September to 2 October transcript was read directly and reduced to an ignored
local review of 119 user turns. It includes the missing-vector request, carrier/refuelling
examples, native-refactor correction, crash/sky/UI/colour retests, failed cloud attempts,
final clean-image acceptance and later FSR4/interop acceptance. Current source/research separates
implementation from suggestions and disproven hypotheses. Raw transcript/captures are not tracked.

Source review covered `games/ac7/src/native_renderer.cpp`, `truesky_depth.cpp`, native scope
contracts and current motion/colour/interop research. The engine checkout was verified at
`0a14a8d537a31ecc77488ced41dbaa0166612ef8`. Read-only installed-file hashing matched the
TrueSky DLL SHA-256 `c6c58ea3a355ed345888b1528e84aa753c6fc69db9c8fb8789e729c7651004d0`
and effect SHA-256 `07eca6e47c1506d5b63236761a97dcf5cdb879480203a63588e978f1210a4d09`.
That is fingerprint confirmation, not a fresh game run or function-execution test.

The skills retain graph roles, exact active rectangles, copied RHI identities, shared uniform
ownership, raw motion validity, producer precision, coordinated TrueSky dimensions and actual
shader activation. Existing references come first; source hosting/decompilers remain useful when
evidence is missing or online research is explicitly requested. General UE4 concepts are separate
from the 4.18.3/AC7 private source/layout evidence.

## Corrected guidance and runtime use

- Present-time evaluation and texture promotion are historical discovery/compatibility paths;
  the accepted implementation uses native owners and graph insertion.
- UI physical raster follows output size while logical layout and native processing remain.
- The hangar null-uniform window requires copy/AddRef and CPU-recorder synchronization.
- Bloom/exposure ordering, per-producer jitter and SDR versus float radiance are explicit.
- TrueSky's x1 allocator did not supply an x1 shader; compilation did not prove activation.
  Actual Texture2D views, RG32F normalized depth and balanced cleanup are essential.
- FSR4's initial refusal was superseded by guarded device-scoped INT8 execution; actual provider/
  model proof remains distinct from internal FSR3 fallback or measured game FPS.

Native renderer, aircraft/cloud, colour, hook-map, tooling and research-index pages now mark
accepted status while retaining dated experiments. The methodology selects bounded F9 capture
when the active DLSS run cannot use RenderDoc. The skills load only relevant specialist evidence.
They guide later research/implementation; no skill code is loaded into the game's runtime.

## Validation and limits

This is documentation/source review without a new rendering experiment. All four skills pass
the skill-creator `quick_validate.py` checks and client metadata validation. Eleven Markdown
files were checked, with all 96 local references resolving; skill files are UTF-8/LF, without
unfinished placeholders. `git diff --check` passes. PyYAML for the standard validator lives in
ignored `.local/skill-validation-deps`; no global Python installation was changed.

No claim is added for complete weapon/missile/ground-vehicle/cloud motion coverage, other-GPU
AC7 visual acceptance, FG/HUD-less surfaces or Reflex/latency integration. A device replay,
compile result or evaluation count remains distinct from user-run moving-game quality. The
accepted UI/cloud/wing fixes must not be reopened merely because an old note still says pending.
