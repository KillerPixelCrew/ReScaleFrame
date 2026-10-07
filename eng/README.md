# Build, deployment and packaging scripts

These scripts separate build/check work from operations that write to an explicitly named game
installation. PowerShell scripts use terminating errors; build/check helpers also check native
command exit codes. Their
comment-based help describes parameters, outputs and recovery limits (`Get-Help ./eng/<name>.ps1 -Full`).

| Script | Inputs and operation | Writes / validation boundary |
| --- | --- | --- |
| `verify.ps1` | Selected Windows CMake configuration; optional `-VS2026` | Build tree and CTest results, Cargo format/Clippy checks; no deployment or Cargo test execution |
| `build-unity-sr.ps1` | Game managed reference directory, Unity native headers, local SDK/build tools | Managed/native artifacts plus Release overlay under `build/windows-x64/bin/<Config>`; no game launch |
| `deploy-ac7-proxy.ps1` | Stopped AC7 install and an already configured native tree | Builds and runs isolated real-overlay fixture, deploys SDK runtimes, then backs up/copies carrier/plugin/overlay |
| `deploy-ac7-sr-runtimes.ps1` | Stopped AC7 install and local FidelityFX/XeSS SDK roots | Validates DLL signatures, backs up and hash-checks copied files; `-ValidateOnly` writes nothing |
| `deploy-unity-sr.ps1` | Pinned Drag'n Wash install, existing build artifacts and local vendor runtimes | Owned-file update, backups, UTF-16 settings and deployment manifest; verifies original-file baseline |
| `package-unity-sr.ps1` | Existing Drag'n Wash deployment manifest and current Release build | Test ZIP, notices, metadata and hashes under `.local/packages`; leaves the install unchanged |
| `wine-test-prefix.sh` | Installed Proton `files/` tree and a dedicated Wine prefix | Copies DXVK/vkd3d DLLs into the prefix and installs native-only registry overrides; no downloads |

## Build and check

```powershell
./eng/verify.ps1 -Configuration Release
cargo test --workspace --locked
./eng/build-unity-sr.ps1 -UnityManagedDirectory '<game>\DragNWash_Data\Managed'
```

`verify.ps1` checks the shared version, builds all configured native targets, executes registered
CTest fixtures, and checks Rust formatting/lints. Rust tests use the separate command. Optional
hardware fixtures may skip; some older graphics fixtures return success when setup is unavailable.
Use [tests/README.md](../tests/README.md) to interpret coverage. No verification command establishes
game acceptance by itself. SDK locations, managed references and redistribution are described in
[dependencies](../docs/dependencies.md).

The Unity build always copies the Release Rust overlay, even when native/managed configuration
is Debug. The Rust DLL is built independently of CMake. VS2022 and VS2026 presets share the native
output directory and should not be used interchangeably without reconfiguring its generator.

## Deployment recovery

AC7 runtime deployment validates every source before writing and restores files written during
a copy/hash failure. The proxy script's SDK deployment is an earlier, separate transaction;
rolling back the carrier/plugin/overlay trio does not undo that SDK update. Backups and manifests
are under `.local/deploy-backups`. The proxy script checks that `Ace7Game.exe` exists; it does not
itself verify the game's researched fingerprint, which remains a runtime/plugin responsibility.

Unity deployment verifies the researched executable SHA256 and refuses unowned or modified target
files. Its first manifest snapshots original files; later deployments reuse that baseline. Backups
are under the game's `ReScaleFrame/backups`. Failures after writing can leave a partial deployment;
there is no automatic rollback transaction. The final original-file comparison detects a changed
baseline but does not restore it.

## Package provenance

Unity packaging validates the exact deployment file set and its hashes before staging. It then
substitutes current Release first-party DLLs, retains deployed vendor payloads and adjusts package
SR settings. Per-file and ZIP readback hashes establish copy/archive integrity. They do not show
that replacement Release DLLs were built from the recorded commit or were tested in the game.
The generated metadata records pending handheld validation separately. Packaging never executes
the deployment scripts. For AC7 packaging, see [tools/package-ac7.py](../tools/package-ac7.py) and
[tools/README.md](../tools/README.md).

## Wine graphics fixtures

```bash
eng/wine-test-prefix.sh --proton '<installed-proton>/files' --prefix '.local/wine-test-prefix'
ctest --preset linux-cross-dxvk
```

Use a dedicated prefix: the setup replaces its graphics DLLs and registry overrides. Without an
explicit Proton path the last matching installed path wins; this is not a semantic version sort.
The default DXVK CTest preset uses `.local/wine-test-prefix`; a custom prefix needs matching launch
environment variables. Cross-built Wine results remain distinct from Windows/MSVC and game runs.
