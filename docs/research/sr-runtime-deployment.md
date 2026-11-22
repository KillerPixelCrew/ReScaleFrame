# FSR and XeSS switching refusal: missing deployed runtimes

30 September 2026. AC7 log inspection, signed-file deployment and a Windows hardware fixture.
No new AC7 gameplay or visual validation was performed for this fix.

## Question and evidence

The user reported that selecting FSR or XeSS in the overlay said refused. The existing game log
records FidelityFX selections 2/3/4 and XeSS selection 5 failing with Windows error 126, followed
by backend error `-4` (LOAD_FAILED). The selector preserved effective backend 1 (DLSS).
The installation contained `ReScaleFrame/streamline`, but no `fidelityfx` or `xess` directory.

This is a deployment omission. The local vendor SDKs and SR implementations existed, but the
proxy/overlay deployment copied only the two first-party DLLs. The common SR loader uses absolute
paths with DLL-load-directory/system search flags. It could not load the absent vendor DLLs.
No renderer hook, motion shader, SR context or fallback behavior was changed for this fix.

## Fix and provenance

`eng/deploy-ac7-sr-runtimes.ps1` installs the unmodified SR binaries and bundled license/notice
files under the game-relative default SDK directories. It validates all source files before
writing, verifies DLL signatures, requires AC7 to be stopped, backs up replaced files, checks
destination SHA-256 and restores written files if deployment fails. `-ValidateOnly` checks the
inventory without changing the installation. Verification never calls this deployment helper.
The local proxy deployment helper invokes it after its build/overlay checks.

| Deployed SR binary | SDK source | SHA-256 |
| --- | --- | --- |
| `ReScaleFrame/fidelityfx/amd_fidelityfx_upscaler_dx12.dll` | FidelityFX SDK 2.3.0, `60f4ea81909200d8542eca14dccb2628b763a9a3` | `d0dcccc74a43c44ba435b7a369b456e0970d8a4464e4bd683119b374f2c9fb46` |
| `ReScaleFrame/xess/libxess.dll` | XeSS SDK 3.0.2, `8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0` | `251659dd84a3e84de67c886a4186e01f3eca49b00641906fe38bb6b807e5d5b7` |

The AMD package exposes the supported FSR2/3/4 implementations through one versioned DLL;
per-family runtime overrides remain available for separate SDK packages. Installing the DLL
does not establish FSR4 hardware support. Only SR binaries were added. Existing proxy, overlay,
Streamline, game configuration and game executable were retained during this repair.

## Checks and limits

Source signatures were valid and all copied file hashes matched. Before deployment the existing
compatibility-pipeline hardware fixture passed against the vendor SDK directories. After
deployment it passed against the actual installed Streamline/FidelityFX/XeSS directories on
NVIDIA vendor `10de`, device `2860`: DLSS -> FSR2 -> FSR3 -> XeSS -> DLSS, with three accepted
synthetic evaluations per selection and successful shutdown.

The D3D11/D3D12 numeric bridge fixture also passed against the deployed directories: FSR2 ->
FSR3 -> XeSS -> FSR2, three frames per selection. Centre red values were 0.249878..0.250244 for
the constant 0.25 input. An explicit FSR4 request returned NOT_SUPPORTED (`-7`) and retained FSR2,
confirming that its hardware refusal is different from the repaired missing-DLL failure.

This resolves the observed missing-DLL cause. It does not establish AC7 FSR/XeSS visual quality
or correct game motion/exposure, which are separate work. The known FSR4 NOT_SUPPORTED result
on this adapter is also separate from Windows error 126. A fresh AC7 run must verify its menu
switching and render results.

Local artifacts: `.local/sr-switch-refusal-preflight.log`,
`.local/sr-switch-refusal-deployed.log` and `.local/deploy-backups/ac7-sr-*/deployment.json`.
The numeric fixture log is `.local/sr-switch-refusal-numeric.log`.
SDK files and game logs remain untracked. See [SR switching research](orchestrator-sr-switching.md).
