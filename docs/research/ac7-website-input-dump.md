# AC7 website input dump

26 October 2026. Source implementation and synthetic validation, with user-run AC7
capture acceptance pending. The question is how to export every available SR/FG
input, native HUD boundary, and final image as named opaque PNGs alongside lossless data.

The existing F9 path is a bounded motion investigation with Targa previews and
intermittent samples. It does not provide a complete browser-ready scene bundle.
The F10 path uses the existing native leased execution callbacks instead of resource
matching at Present. No new binary offsets, engine patches, or dependencies were added.
PNG encoding uses the Windows Imaging Component already provided by Windows.

The AC7 carrier fingerprints the actual executable with its existing SHA-256 helper
and configures the exporter. Native SR begin selects one primary session, source frame,
family, view, and native frame. The pipeline exports the rect-local jittered HDR source,
encoded/decoded motion, original depth and available guides. DLSS exports its final
assembled D3D11 inputs before the shared-D3D12 translation, including the reset decision.
The FSR/XeSS adapter exports its resolved depth/pixel motion before bridge submission.
Backend output is captured before transport decode, then again after optional colour
correction at native linear-HDR reinsertion.

The native HUD composition callback exports its borrowed scene/UI inputs before drawing
and its readable output after drawing. Final-scene completion exports the surface handed
to FG. Normalized FG depth/motion are read at the existing FG capture handoff; the actual
provider constants are serialized at preparation when eligible. The matched native window
Present exports the final game image before debug view/RSF overlay drawing. Present
completion closes the manifest after provider preparation. Different frame/view identities
cannot fill holes. Absent optional inputs and incomplete required stages remain explicit.

Readback occurs synchronously on the immediate-context owner while each resource is still
leased and before later writes can change its contents. No borrowed texture is retained.
Staging copies preserve graphics bindings; the exporter temporarily disables predication
and restores its exact predicate/value afterward. Readbacks and file encoding can hitch
the selected frame. This path does not change SR/FG configuration or launch/stop the game.

Raw rows preserve native DXGI storage, including HDR, alpha, sparse sentinels and
nonfinite values. Camera JSON represents nonfinite scalars as null. PNGs are opaque;
HDR, logarithmic depth, motion, mask and alpha mappings are documented in the manifest
and [capture instructions](../../loader/README.md#ac7-website-input-dump). The native HUD
raster and pre-window FG scene are distinct from a guaranteed extracted UI layer; the
current FG API receives no separate UI texture. A still bundle does not expose internal
vendor history or generated images.

Validation uses a hidden WARP window with known HDR, depth, mask, motion and HUD pixels.
It independently decodes PNGs with WIC, verifies every alpha byte is 255 and the expected
transforms, compares raw HDR bytes including alpha/NaN/Inf, refuses deferred contexts,
and rejects HUD/Present from another source frame. It is a synthetic handoff test, not
an AC7 run or a claim about visual quality, pacing, or generated-frame acceptance.

Delivery evidence: source baseline `9699b4d` with pre-existing unrelated workspace
edits preserved. The installed executable matches SHA-256
`c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`.
Windows/MSVC Release build and the WARP fixture pass, including predicate restoration
and FG constants recorded between Present begin/completion. A pre-existing refusal
test used R32_FLOAT, now intentionally supported for scalar depth; it now tests the
unsupported R32_UINT format. The corrected full native batch reports 68 passes,
11 opt-in skips and one separate vendor-census hook-installation failure. That fixture
passed in the first batch and links neither the exporter nor the graphics library.
Its source was left untouched. Rust formatting and Clippy pass separately.

The established AC7 deployment script passed its real panel/Insert/render/resize
fixture and backed up, copied and hash-verified the proxy, plugin, overlay and SDK
dependencies with AC7 stopped. Logs: `.local/ac7-website-verify-fixed.log` and
`.local/ac7-website-deploy.log`; deployment manifests are under
`.local/deploy-backups/ac7-pair-20261026-191217-390` and
`.local/deploy-backups/ac7-sr-20261026-191216-875`. User-run F10 capture and visual
acceptance remain unverified; no game was launched or stopped for this work.
