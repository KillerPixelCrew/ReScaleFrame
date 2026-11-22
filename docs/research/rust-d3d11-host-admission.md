# D3D11 host admission and output leaf port

D01 updates the shared Runtime host at baseline `1150dcbb` without changing
engine hooks, resource producers, presentation order, or reduced-frame recovery
policy. This is source evidence. Builds, tests, device fixtures, and live games
were reserved for the user.

- The host accepts the 160-byte ABI 8 setup prefix and defaults its far plane
  from the legacy inversion flag. The current 168-byte setup carries its own
  finite or infinite convention. The host passes both flags independently to
  presentation. Partial extensions and unknown conventions are refused.
  Skyrim's existing prefix remains valid. Fallout uses regenerated bindings.
- Common SR admission checks the session before graphics acquisition. Reports
  take colour, depth, motion and validated camera jitter from the pass sent to
  evaluation. A configure-only pass preserves the prior input receipt.
- CPU diagnostics and dispatch use one bounded decoder for the legacy and
  complete source packet shapes. Diagnostics retain the raw header and result
  for malformed packets. Steam and the Runtime now share the generated chain
  dispatch and use its named Present arguments and HRESULT result.
- A shared surface key covers texture, kind, flags, minimum record size and
  output rectangle. Auxiliary products still validate source mapping and
  jitter. The host calls the typed Rust products owner directly. The native
  auxiliary fixture still uses its C++ wrapper.
- A descriptor per advanced option drives the overlay, INI operations and
  actions. Capture controls use the named option identifiers.
- Thirteen D3D11 output leaves now use typed Windows bindings and retain the
  original C symbols for native callers. Returned COM pointers carry one
  reference. The host's public forwarding exports now name the Rust functions
  directly, while the C++ create entry provides the native platform table.

## Anomaly backend handoff

`rsf_d3d11_sr_admit` takes the caller's concrete backend mask and vendor
directories. A call without a device supplies package choices and an Auto
default for overlay setup. A call with the engine's D3D11 device resolves Auto
or an unavailable saved choice through the existing Runtime GPU preference.
Package evidence says which runtime files exist. Provider startup still owns
SDK loading, hardware support and plan admission.

Integrated D05 commit `6c01497a` supplies the Runtime host's selected-provider
start record and uses provider-specific startup. D01 exports that same entry
point and the Anomaly driver binds it by name. A Runtime that exposes it starts
the requested provider directly. The legacy API table and its DLSS-first
startup remain available to older consumers. The Anomaly overlay's Auto row
and its use of Runtime package and device admission remain follow-up work.

## Fixture and native boundaries

Synthetic Rust fixtures cover all depth-direction/far-plane combinations,
foreign-session refusal, SR input receipts through configuration, CPU packet
sizes, package/GPU preference, and direct alternate-provider startup in the
Anomaly driver. The public DLL contract fixture also probes
the legacy/padded setup boundary and a foreign session with unreadable graphics
sentinels. The two device fixtures explicitly request the infinite far plane
they relied on before.

`d3d11_host.cpp` still owns the create table and native exception guards around
presentation, SR pipeline operations, and capture callbacks. The public and
private headers describe live C contracts. The three native fixtures still use
the DLL import surface or their hardware SDK and presentation harnesses.
Their port remains pending.

## Residual replay on `84a8b9eb`

- The production host table now uses typed Rust implementations for COM identity
  and Release, the D3D11 immediate context, DXGI adapter vendor and swapchain
  queries, the diagnostic Present address, and INI access. The old C++ calls
  in `d3d11_host.cpp` supply the comparison. Borrowed receivers stay borrowed,
  successful COM queries transfer one owned reference, and Release returns
  the original count. The adapter vendor query is shared with package admission.
- `sdk/game/src/d3d11_host_abi.rs` now generates the remaining public Present,
  presentation binding and capture-control records from `d3d11_host.h`.
  Runtime and fixture use the same records. The C contract launcher still
  compiles the public layouts and exercises the two C-inline frame predicates.
  The Rust contract fixture links the production Runtime exports and carries
  the old setup, cold-host, recovery, capture and dispatch assertions, including
  refusal of unreadable receivers and C04's scoped and unidentified FG leases.
- Both hardware fixtures now share Rust texture allocation and CPU event
  construction. The rendered-FG fixture still declares render expectation only
  at frame end, and the shared-SR fixture leaves that field clear. Shared-SR
  auxiliary snapshots use a Rust staging map and pack rows using the reported
  pitch. The existing C++ scenarios and diagnostic checks remain hardware
  fixture port work.
- D05 provider startup still forwards the requested backend in the host
  callback. Anomaly's package and device admission, Steam's tagged dispatch
  ABI, the exact native chain continuation, and C04 FG callbacks and lease
  decisions remain as integrated at the base.

The C++ host still publishes the create entry and the native callback table,
including exception containment for callbacks into C++. The private platform
header still defines that table. The output platform header still supplies
bindgen input and the C leaves used by the native SR pipeline. The Rust contract
body and public host ABI are source-ported. The two hardware fixture bodies
are partial ports. No hardware behavior was verified in this replay.
