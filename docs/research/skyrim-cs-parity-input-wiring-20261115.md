# Skyrim CS parity input wiring, 15 November 2026

Question: restore the verified colour-default and temporal-mask input differences
without assuming that either caused pickup ghosting. Initial wiring was source
work only; later coordinator verification and bounded observations are recorded
below, including the subsequent review-fix gate and bounded resource run.

Evidence and scope follow [the reference parity audit](skyrim-sr-reference-parity-20261115.md),
for SkyrimSE 1.7.104, SHA256
`846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f`,
and CS `038a0a04ce9db5b84707ec662ddfe7824cdac3dd` (pinned comparison `265c28f5`).
The independent Fable audit supplied the corrected v4 producer evidence:
`.local/skyrim-research/prop-draws-20261115-223539/native-draws/manifest.json`
contains 22 census entries, 465 draws on targets `[1,85,3]` in two frames.
This corrects the older no-producer claim for target85. It does not establish
target85's values at SR, pickup coverage or a missing native motion producer.

`render_inputs.cpp` now reads target85 from the same native target table as
main colour1 and motion7: module base + `0x3331f40 + 0xa58 + index*0x30`.
The existing guarded pre-HDR main-camera callback leases it with a ComPtr until
the synchronous SR callback returns. The checks require the context's device,
full native allocation dimensions, one mip/slice/sample and typed numeric
two/four-channel float, UNORM or SNORM format. The review correction makes
missing or rejected masks optional: null with zero weights preserves ordinary
SR. The later runs measured R8G8_UNORM; successful native-value readback is recorded below.
AddRef protects allocation
lifetime only; shared processing must snapshot before native postprocessing
reuses it, as with the other SR inputs.

Game ABI22 carries the mask under the same frame/view/history identity and
top-left active render rectangle as SR. Adapter weights `(0.1,1,0,0)` reproduce
the inspected CS X/Y rejection expression; the shared implementation crops and
computes `saturate(dot(channels,weights))` into R32, then maximum-composes with
independent reactive/bias inputs. No TRANSLUCENCY scopes or coverage are invented.
Host ABI5 selects native colour by `colour_transport_default=1`; a saved overlay
choice takes precedence. This restores a reference default for comparison and
does not establish that the former encoded SDR contract was invalid.

The main-colour format guard also admits `R16G16B16A16_FLOAT` beside the observed
`R11G11B10_FLOAT`. This is a proven source-level format need: CS HDRDisplay's
DataLoaded enables `bUse64bitsHDRRenderTarget`, and RedirectFramebuffer supplies
a float16 texture. The shared native-region conversion already accepts float16.
The retained vanilla F10 frame is R11G11B10, so float16 is not demonstrated as a
need for that capture or pickup. Admission applies only to the SR main-colour
reader. Presentation readers and CS HDR ownership/colour-space refusal remain
separate, unchanged requirements documented in
[the CS handover note](skyrim-cs-handover-20261115.md). This does not enable CS HDR.

Initial validation was source review and `git diff --check` only. Motion conditioning,
effective model parity, raw mask contents and attributable pickup frames remain
open. No native history reset or shader replacement was added.

Separate prior timeline evidence is retained in
`.local/skyrim-research/input-acceptance-20261115-231558` (`run.json`, `loader.log`,
`exit.json`, `restoration.json`). That bounded run used source `54235df` plus
the SIMULATION_BEGIN/capability connection, runtime SHA256
`A4C742CB3C17C2FE0999EE84EA6E16770CF057643D67F6DBC671D1B22A588D31`
and entry SHA256
`90834FD03943CFA9DD32C125E3CE70B8EE43EBDD6E164CE884B6A2FFD7C626AD`.
It reported DLSS FG active1, Reflex effective1, 333 tags and 676 provider
presentations, exited normally with code0, and verified restoration of Runtime,
Entry and INI. These are bounded timeline/provider observations, not measured
latency or physical display cadence. The run predates the mask, native-colour
default and float16-reader changes in this note, so it does not validate them.

The subsequent parity trial
`.local/skyrim-research/parity-acceptance-20261115-232954` stopped before prepare:
its loader log contains only `Plugin prepare result=4294967294` (ABI mismatch,
signed -2). The validation controller had replaced Runtime/Entry but left the
old root XInput carrier, whose `plugin_carrier.cpp` embeds the game ABI in host
services. Skyrim validates those services before DPI setup, so this refusal
also precedes its DPI/window preparation; the reported render outside the window
does not establish a scaling regression in the new path. The ignored controller
now backs up, deploys and hash-checks the matching built `xinput1_3.dll`, includes
its hash in the run receipt, and restores/hash-verifies the original alongside
Runtime/Entry/INI after orderly owned-process exit. Startup now checks the log
every 250 ms and fails early on any nonzero prepare result. Existing-process
refusal and deferred restoration if orderly exit fails remain intact. This
controller correction has parser/source checks only and no new rendering
acceptance; the coordinator must rerun bounded validation.

The matching-carrier rerun PID44520, retained under
`.local/skyrim-research/parity-acceptance-20261115-233223`, exited with code0
and restored all four files. Its `run.json` records Runtime SHA256
`85C32AF0038BC02FD8B7F3F74F15EC989F153C71A8FC8ECB0FF6D6E1091D3B27`,
Entry SHA256 `88F9E084BA0EC1EFF16D453400CBE6958FDE9D1532977B5D359EBDAC14BB877B`
and carrier SHA256 `8AC3813EB170570817011A3C4C949D841D223A1944450126A6267357F292B66D`.
This followed the coordinator's Windows Release gate,220 passed/14 skipped.
The coordinator inspected the final image: Riverwood renders behind the initial
Survival Mode Yes/No modal, so these are menu-scoped SR observations, not ordinary
gameplay, input or pickup acceptance. SR returned0 with matched Present0;
DPI API accepted1 and output texture1920x1080 were observed, but physical client
bounds were not measured.

`mask-analysis.json` records `color_encoded=0` and exact equality between
`sr_source_jittered` and `sr_color`. Normalized reactive and bias masks are
identical1280x720 R32, finite0..0.10000000149, with82,949 nonzero pixels (9%).
The F10 manifest is **partial**: native target85 is1920x1080 DXGI49 R8G8_UNORM,
but raw/preview readback returned -3 (unsupported dump). The shared worker owns
that narrow dump support correction. No comparison against native X/Y values
or pickup cause is established. Review fixes in
[the review note](skyrim-review-fixes-20261115.md) postdate this earlier run;
the subsequent combined verification follows below.

## Complete native-mask comparison after review fixes

The coordinator's combined `eng/verify.ps1 -Configuration Release -VS2026`
gate exited0:220 passed/14 skipped, Cargo fmt and Clippy clean. Retained log:
`.local/skyrim-research/verify-skyrim-opus-fixes-20261115.log`. This verifies
the concrete review-fix and raw-mask dump-support batch, not optional architecture
proposals or broad mod compatibility.

The bounded rerun PID5484 under
`.local/skyrim-research/parity-acceptance-20261115-234222` completed F10,
exited0 and hash-verified restoration of Runtime/Entry/INI/carrier. Source
baseline was54235df plus the uncommitted trial changes; `run.json` identifies
Runtime SHA256 `6FD744B77ECEAD29A11AC2AA87F5BFBFAE3D546D95D631C11A9D14E3A2F89D19`,
Entry SHA256 `268B4B10AF67D46B87FCB4B26DF2A260E0E2E7CED62A3A0EBAA85294D504630D`
and carrier SHA256 `8AC3813EB170570817011A3C4C949D841D223A1944450126A6267357F292B66D`.
Capture `scene-20261115-234307-869-44503546-5484-1` identifies source1593,
native frame1829, SR result0 and matched Present result0.

`mask-analysis.json` records native DXGI49 R8G8_UNORM1920x1080, cropped to the
top-left active1280x720. Both R32 reactive and bias masks equal
`clamp(0.1*R+G,0,1)` exactly, maximum error0. All values are finite in
0..0.09921568632125854;83,422 pixels are nonzero. `color_encoded=0` and
`sr_source_jittered` equals backend `sr_color` exactly. This completes the native
X/Y comparison that the previous unsupported-dump capture could not provide.
`window.json` independently measures physical client1920x1080 at144DPI,
matching output1920x1080; outer window bounds are[0,0,1942,1136].

The Survival Mode modal remained after the coordinator's attempted click.
SceneNotEligible with no FG tags confirms refusal under that menu, not gameplay
FG or pickup acceptance. This run validates the bounded native-colour/mask and
client/output contracts after the fixes; it does not exercise every lifecycle
failure branch, prove lower latency or resolve pickup ghosting. ENB, SKSE and
SSEDisplayTweaks coexistence testing is the coordinator's next authorized task;
no new RSF SKSE dependency or mod-compatibility acceptance is claimed here.
