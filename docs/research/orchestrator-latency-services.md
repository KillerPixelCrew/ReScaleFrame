# Independent PCL and Reflex services

30 September 2026, following FG foundation commit `38e5aaf`. This extends the reusable runtime;
it does not install AC7 input hooks or claim measured physical latency.

## Approach and implementation

FG is optional, so its provider cannot be the only owner of latency events. The early Streamline
host now exposes independent begin, semantic marker, abort, Reflex configuration and bounded
status operations. DLSS-G uses the same operations rather than minting another token or sleeping
twice. Host ABI 2 appends explicit cold-start feature profiles and optional shared DLSS SR loading.
The FG provider ABI remains 1; game plugin ABI remains 1 and its new CPU/frame callbacks are pending.

- DLSS-FG profile requests DLSS SR, DLSS-G, Reflex and PCL.
- Reflex profile requests SR, Reflex and PCL without FG.
- PCL profile requests PCL alone unless DLSS SR is explicitly requested. It neither initializes
  Reflex nor calls its sleeper. This avoids requiring NGX or NVIDIA graphics for CPU telemetry.

Each real source frame mints one SDK token. Six ring entries retain the SDK's mutable token objects
until PresentEnd or abort. Neutral IDs above 32 bits map to a separate monotonic SDK sequence.
Reflex Off still sleeps once per frame when the feature is loaded; On and Boost are separate modes.
The limiter is initialized independently. Ordinary input sampling is recorded by the runtime's
CPU ledger; only an actual controller sample is forwarded as PCL marker 13. Marker 6 is not sent.

The host's chain-upgrade helper recognizes already upgraded factory output, retaining one owned
COM reference. A plain upgraded chain supplies common-present bookkeeping even with no FG provider.
Configuration/status are graphics-owner-thread operations; marker and begin use the token lock.

## Device experiment

`tests/latency_host.cpp` creates a hidden 256x256 plain D3D12 chain, samples Space, updates a small
simulation value, renders a changing clear color, submits, presents and drains its queue for
12 source frames. It marks the actual boundaries of that fixture. Logical IDs are
`4294967297..4294967308`, deliberately above the SDK's 32-bit range. Reflex changes Off -> On -> Boost
in groups of four frames. The ordinary keyboard input event never becomes a fake controller event.

| Adapter / profile | API evidence |
| --- | --- |
| NVIDIA `10de:2860`, PCL-only | Host/chain/render/Present/teardown pass; 0 sleep calls, 72 successful phase marker calls; last neutral ID 4294967308 |
| NVIDIA `10de:2860`, Reflex | Off/On/Boost configuration and same rendering lifecycle pass; 12 sleep calls, 72 successful phase markers; SDK low-latency and latency-report availability true |
| Intel `8086:4688`, PCL-only | Same PCL/render/Present/teardown pass; 0 sleep calls, 72 successful phase markers; no Reflex/latency report claimed |

Counters instrument successful host SDK calls, not an independent ETW/NVAPI measurement. The SDK
report-availability flag is not a physical input-to-photon result. PrintPCL, ReflexTest, ping/flash
integration, external instrumentation and game hooks have not been validated here.

The first PCL-only profile unnecessarily requested DLSS SR, producing an expected missing-NGX
warning on Intel. PCL-only now requests no NGX feature unless SR is explicitly requested; the
repeated Intel fixture passes without that error. All runtimes come from the signed local
Streamline 2.14.1 package. Matching inspected source is
`2122257e0fce486f91b385aa63b9a09b0a34b363`.

## Source correction and remaining work

Streamline's `PluginManager::setFeatureEnabled` changes the enabled flag and rebuilds hook lists;
it does not physically unload the DLL or call feature startup/shutdown. PCL caches a Reflex marker
function from shared data. Consequently a feature-load toggle is not proof of a clean CPU-only
PCL/XeLL profile transition. Fresh cold profiles are explicit; hot ownership transitions still
need source/device validation, with restart refusal retained where that proof is missing.

The configured Ghidra service remains offline. AC7's [static CPU leads](ac7-fg-cpu-boundaries.md)
still need runtime identity/renderer handoff validation. Shader/motion work stays with its separate
owner. Full native verification initially encountered concurrent texture-dump API edits. After
those call sites settled, the Release VS2026 gate passed: 30 tests passed and five opt-in hardware
fixtures skipped. SDK-free C-header/controller/lease checks also pass; vendor fixtures skip there.

Ignored logs: `.local/latency-pcl-nvidia.log`, `.local/latency-reflex-nvidia.log`,
`.local/latency-pcl-intel.log`, `.local/latency-release-verify.log`, `.local/latency-no-sdk-build.log`.
