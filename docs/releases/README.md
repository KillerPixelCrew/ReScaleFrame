# Building one RSF release

RSF ships as one release per RSF version, built from one committed tree. The `vX.Y.Z` GitHub release contains AC7,
RDR2, Anomaly and Drag'n Wash ZIPs, the Minecraft JAR under its own mod version, one corresponding-source ZIP and
one `SHA256SUMS.txt`. Project Wingman is observer-only and is excluded. Existing per-game tags remain historical;
do not create new ones. The Unity and Godot integration source remains in the source archive.

Fetch and merge main before preparing a release, then commit the intended changes and aligned VERSION/Cargo version:

```powershell
git fetch origin
git merge origin/main
```

Use Python 3.11 or newer. Run the native/Rust gate and the integration checks before packaging. A clean tree is mandatory, including for the
release-set dry run. The tool checks that the fetched `origin/main` is merged, but cannot prove the remote was just
fetched. It never fetches, merges, commits, pushes, deploys to a game or publishes a release itself.

```powershell
py -3 tools/package-release.py --streamline-root D:/Coding/ReScaleFrame/.local/streamline/sdk --dry-run
py -3 tools/package-release.py --streamline-root D:/Coding/ReScaleFrame/.local/streamline/sdk
```

Default output is `build/releases/<VERSION>`. Use `--output <unused-directory>` to choose another ignored or external
directory. Existing destinations are refused. The builder creates a private staging directory, invokes the existing
integration packagers, checks their source identities, retains only their game downloads, and publishes the complete
directory after the source and inputs are checked again. Failure leaves no final release set. Standalone per-game
commands retain their existing outputs and clean-tree rules.

Without `--build`, every required first-party Release output and the exact Minecraft JAR must exist and have a
timestamp at least as recent as the commit. Refusal lists every missing or older file. This is a freshness check,
not a cryptographic claim about a compiler. The embedded Minecraft native payload is checked against its manifest,
the current native DLL and the supplied Streamline payload; the staged Unity overlay must match the Rust overlay.
Do not package while another process is changing the build outputs.

An explicit build is available when the normal build directory is free:

```powershell
py -3 tools/package-release.py --streamline-root D:/Coding/ReScaleFrame/.local/streamline/sdk --build
```

This builds Cargo Release, native Release, the matched Unity Mono helper, copies the overlay, stages Minecraft's
native payload and runs Gradle build with one worker and a 768 MB JVM heap. The CMake tree must already be configured.
Unity requires prepared public 6000.3.14f1 references, default `.local/unity-references/6000.3.14f1`; override with
`--unity-managed-directory`. Minecraft's staging script uses the checkout's prepared SDK locations. Its resulting JAR
must match the Streamline payload passed to the release builder. No game installation is a build input.
Successful incremental builds may reuse older unchanged binaries; the JSON summary lists them. `--dry-run` and
`--build` cannot be combined. Never run this build option in a directory another worker has reserved.

The tool writes one `ReScaleFrame-<version>-source.zip` using `git archive` of the recorded commit and adds the
incorporated yyjson source/header/license after checking their pinned hashes. It never adds local game shader trees,
licensed engine source or compiled caches. Archives receive CRC/path/duplicate checks and asset hashes are streamed.
The JSON summary reports version, commit, tag, names, sizes and SHA-256 values. A dry run reports planned names with
null sizes/hashes and creates no release files.

Review `release-notes.md`. Fill in each integration's **Played on this build** line with what was actually played,
including hardware/path limits. The generated default is **not yet played** for all five integrations. Update each
short player-facing change summary as appropriate. Notes are local publishing metadata, not a download asset, so
editing them does not invalidate the checksums. The checksum file covers the six game/source download assets and
does not hash itself.

After reviewing the asset hashes and notes, the coordinator publishes the recorded commit, never a moving branch:

```powershell
$release = 'build/releases/X.Y.Z'
$assets = @(Get-ChildItem -LiteralPath $release -File | Where-Object { $_.Extension -in '.zip','.jar' } | ForEach-Object FullName)
$assets += Join-Path $release 'SHA256SUMS.txt'
gh release create vX.Y.Z --target <commit> --title "ReScaleFrame X.Y.Z" --notes-file "$release/release-notes.md" @assets
```

Verify that the tag, recorded commit and every upload belong to this one release set. The packager does not run the
publication command. If anything changes, use a new empty output directory and build/check the full set again.

Packaging orchestration and the existing packager regressions can be tested without release binaries or a game:

```powershell
py -3 -m unittest discover -s tests -p 'package_*test.py' -v
```

The orchestration fixtures use private Git repositories, tiny mock payloads and stub integration packagers. They
exercise the complete set, JSON, notes, single source/checksum files, SDK/JAR identity, stale or dirty inputs,
changed source/build inputs, commit mismatches and failed assembly without publishing a partial directory.
