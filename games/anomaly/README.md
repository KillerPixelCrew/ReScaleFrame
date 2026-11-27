# S.T.A.L.K.E.R. Anomaly adapter

The X-Ray Monolith DX11 renderer integration: native boundary observation, geometry identity, SR stages,
motion MRT shader patching, the FG bridge and input facts. Status and validation are in
[docs/implementation.md](../../docs/implementation.md) and [engine.json](engine.json).

## How the adapter finds its sites

The adapter does not depend on one executable build. At start-up `src/discovery.rs` copies the running
executable's code from live memory once and resolves all 122 sites it hooks, patches, calls or reads:

- 104 sites from byte patterns that each match exactly once in the executable sections of both 1.5.3 DX11
  builds (`AnomalyDX11.exe` and `AnomalyDX11AVX.exe`). Rel32 branches, RIP displacements and the few bytes the
  SSE and AVX builds encode differently are wildcards. Functions are found at their entry or, where the entry
  differs in length between the builds, through their only distinctive call. Globals and vtables are the
  RIP-relative operand of an instruction that uses them.
- `CRender::rmNear`, `rmFar` and `rmNormal` from slots 55 to 57 of the discovered CRender vtable,
  `CSkeletonX_PM::Render` from slot 7 of its vtable, and the position binder's `setup` from slot 0 of the
  vtable its statically initialised object holds (found from the binder registration's `lea r8`).
- The six first-person `set_P`/`rmNear` call pairs, decoded from the functions that hold them.

Every site's expected code is matched at the found address: its pattern, or for a site found through a caller
or a vtable slot its own check (one encoding per codegen where SSE and AVX differ in length, for example the
position binder body RSF's replacement mirrors). The Fvisual phase compare and `IsSVPFrame` must also read the
discovered phase and frame globals. The profile
records the hooks and guards consume (`native_profile.rs`, `geometry_profile.rs`, `sr_profile.rs`,
`motion_profile.rs`, `input/profile.rs`) are built from the found sites; a detour's 16 expected bytes are
copied at discovery and compared again before MinHook writes it.

A missing, ambiguous or changed site refuses only the feature it serves (native observation, geometry, SR,
motion or input) and the adapter and session logs name the site; the game keeps running. The executable's
SHA-256 is logged as evidence. Neither it nor the file name selects or admits anything.

Structure offsets that no code reference reveals (CRenderTarget slots, CRT/CTexture/RCache fields, the
shader_compile caller frame) stay documented constants in the profile files. Most are fixed bytes of a
discovered pattern, and `tools/anomaly_*_evidence.py` re-derive each from the executable's code.

## Evidence and tests

- `cargo test -p rsf-anomaly`: `discovery::tests` resolves every pattern in both installed executables
  (`D:\Anomaly 1.5.3`, or `RSF_ANOMALY_DIR`) and checks the built records equal the recorded ones field by
  field. It is skipped when the game is not installed.
- `py -3 -I tools/anomaly_profile_evidence.py`: runs the geometry, SR, motion and FG evidence on both builds
  against the discovered sites (`tools/anomaly_discovery.py`) and proves that altered patterns, checks,
  guards and layout values are refused.
- `tools/derive-site-patterns.py IMAGE SPEC --also IMAGE` derives a pattern that resolves in both builds.

The recorded records of the two builds (`RECORDED` in each profile file) are test and fixture data behind
the `recorded-profiles` feature, which every fixture feature enables. The shipping plugin never compiles them.
Research: [anomaly-native-hooks.md](../../docs/research/anomaly-native-hooks.md#signature-discovery-24-november-2026).
