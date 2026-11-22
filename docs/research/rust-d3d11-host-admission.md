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

D05 commit `03ed9661` supplies the Runtime host's selected-provider start
record and uses provider-specific startup. Its startup function is private to
the Runtime. Anomaly still needs the admission binding in its overlay and driver
and a public additive route to that startup function. The existing Anomaly SR
table stays compatible during that follow-up.

## Fixture and native boundaries

Synthetic Rust fixtures cover all depth-direction/far-plane combinations,
foreign-session refusal, SR input receipts through configuration, CPU packet
sizes, and package/GPU preference. The public DLL contract fixture also probes
the legacy/padded setup boundary and a foreign session with unreadable graphics
sentinels. The two device fixtures explicitly request the infinite far plane
they relied on before.

`d3d11_host.cpp` still owns the create table and native exception guards around
presentation, SR pipeline operations, and capture callbacks. The public and
private headers describe live C contracts. The three native fixtures still use
the DLL import surface or their hardware SDK and presentation harnesses.
Their port remains pending.
