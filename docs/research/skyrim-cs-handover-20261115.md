# Community Shaders startup handover, 15 November 2026

## Scope and result

Question: can RSF automatically take SR, FG and latency ownership from the
published CS release while preserving its other features and desired settings?
The first pass validated and diagnosed the exact profile but refused handover.
The continuation below implements an exported-initializer boundary within the
adapter, retaining the original configuration load and excluding Upscaling's
effective lifecycle. CS HDR fallback resource ownership remains refused.

The initial pass changed the compatibility files, CMake and prepare/start
checkpoints. This continuation changes only the compatibility files, necessary
plugin gates/status and native-creation admission. CPU and shader files stay frozen.

## Matched evidence

The release source is `references/skyrim-community-shaders-1.9.1` revision
`265c28f5`. DLL SHA256 is
`10ba95e6b1cd6b675ce74d1d681fae41e667bdd8b7b5ba8c362e657511097827`.
PDB GUID `240F7D4A-CB53-466A-B55C-6BCF3E723338`, age 1, was matched in the
earlier release research. This continuation reused that database in IDA session
`949f8ffe`. The raw PE is AMD64/PE32+, timestamp `0x6ab51a9f`, SizeOfImage
`0x144b000`. Timestamp is an identity field, not a reliable build-date claim.
The local evidence is `cs-pdb-evidence.json`, `cs-method-disassembly.json` and
`cs-startup-handover-20261115.json` under `.local/skyrim-research`.

The validator checks the exact file digest, PE identity and executable-section
19-byte entry guards, plus the same mapped instructions if already loaded:

| RVA and role | Expected bytes |
| --- | --- |
| `3a4b30` Feature::Load(json&) | `48 89 54 24 10 48 89 4c 24 08 53 56 57 41 54 41 55 41 56` |
| `4f9380` Upscaling::Load() | `48 83 ec 48 48 8d 05 ad 61 21 00 48 c7 44 24 28 1d 00 00` |
| `4f8ed0` Upscaling::PostPostLoad() | `40 55 53 56 57 41 56 48 8b ec 48 81 ec 80 00 00 00 48 8d` |
| `4f7030` ConfigureUpscaling() | `48 89 6c 24 10 48 89 74 24 18 57 48 83 ec 60 80 3d 1d 21` |

It holds a read-only file handle across hashing and non-executable mapping;
each raw offset is bounds checked. A loaded module gets a temporary module
reference, released after inspection. No LoadLibrary of CS occurs. Unknown,
unreadable or modified profiles refuse, as does the known profile. Identification
does not prove that initialization has not happened.

## Proven lifecycle and additional blockers

The carrier `loader/proxy/src/plugin_carrier.cpp` creates a background startup
thread from DllMain. That thread hashes the executable, loads the runtime and
prepares/starts the game adapter. There is no synchronization with SKSE's plugin
initializers. Hashing or thread scheduling cannot be used as load-order proof.

SKSE reference revision `25b72352adb6543fa6d0bd3795780672b2e238e0`,
`skse64/PluginManager.cpp:143`, shows LoadLibrary followed by GetProcAddress
and SafeCallLoadPlugin on the loading thread. This is source evidence for the
ordering requirement, not a matched installed SKSE binary or a patch profile.
Neither SKSE nor CS is installed in the current game.

The shipped CS `SKSEPlugin_Load` export is RVA `631800`. Native calls initialize
SKSE at `631edb`, register the message listener at `631f78`, initialize globals
at `631f7d/631f82`, then call State::Load at `631fa1`. At `632516`, the native
feature loop tests `Feature.loaded` before virtual Load at `63251f`. Upscaling
object `13dcf40`, loaded byte `+8`, settings `+58` agree with PDB evidence.
Its Load `4f9380` installs the D3D IAT wrapper; that wrapper calls
LoadUpscalingSDKs before device creation. PostPostLoad `4f8ed0` installs native
jitter, resolution and postprocess changes. Changing settings alone leaves
these owners in place, including ConfigureUpscaling ratio overrides in TAA/None.

Two preservation details matter for the next implementation:

1. Skipping Feature::Load(json&) completely loses desired in-memory settings.
   In release source, State::SaveToJson unconditionally invokes Upscaling's
   SaveSettings and then every feature's Save, regardless of loaded. A later
   CS save could serialize defaults over the user's desired settings. The
   appropriate future hook is void `(Feature* RCX, json* RDX)`: forward original
   configuration loading, then force only this exact Upscaling object's loaded
   byte false before returning to the lifecycle dispatcher. LoadSettings at
   `4f9500` deserializes/clamps settings and reads the native TAA preference;
   it does not install Upscaling lifecycle hooks. Repeated settings loads must
   retain the exclusion. No STL/json ownership crosses into RSF.
2. With Upscaling absent, native early-hooks code takes its fallback at
   `632372` and patches the core D3D creation IAT at `63240b`; the DXGI factory
   IAT is patched at `6324a5`. These core hooks preserve other CS behavior and
   are not the Upscaling SDK wrapper. HDRDisplay::PostPostLoad independently
   installs interface/postprocess fallback hooks when Upscaling.loaded is false.
   Suppressing Upscaling is therefore not equivalent to restoring vanilla hook
   bytes or render formats. Current RSF native call guards must still refuse
   unexpected CS fallback ownership until those wrappers are explicitly chained
   and their color/HDR resource contracts are validated.

CS core input dispatch calls UpdateReflex unconditionally. The shipped
UpdateReflex is `504d40`: native/decompiled entry checks initialized, adapter
support, featureReflex and the options function pointer, not Feature.loaded.
Only exclusion before SDK initialization can leave this harmless without
adapter vendor calls. A zero loaded byte is not evidence of absent SDK state.

## Required startup contract

An implementation needs these ordered phases, outside the loader lock:

1. Establish a barrier before CS's SKSEPlugin_Load can execute, identify and
   retain the exact mapped release, and install the guarded Feature::Load
   configuration-preserving exclusion before releasing the initializer. Reject
   an already entered initializer or uncertain prior ownership. A background
   poll, delay, DLL-name ordering or an asynchronous loader notification cannot
   substitute for this barrier. Do not run MinHook thread suspension, file
   hashing or vendor work inside DllMain/loader notification callbacks.
2. Let original CS initialize other features. Confirm Upscaling lifecycle and
   SDK initialization stayed excluded, and capture CS's completed fallback-hook
   topology before RSF native hook preparation. Preserve HDRDisplay honestly;
   unsupported fallback resources or chain order must still refuse activation.
3. Keep the exclusion process-lived across CS settings reloads and RSF session
   stop/start. Re-enabling CS Upscaling after its normal startup window requires
   a separately proven lifecycle, not restoring the loaded byte. Desired CS
   settings remain untouched throughout. An RSF initialization failure must
   refuse rendering rather than belatedly starting CS SDKs.
4. Apply the same synchronous arbitration to later loads before their feature
   initializer executes, and quiesce RSF resource/presentation ownership first
   if necessary. No current generic startup ABI expresses this contract; no
   game-named policy belongs in the generic carrier/runtime. A matched SKSE
   loader interception profile or an upstream cooperating CS API could supply
   the game-side barrier without adding an SKSE runtime dependency to RSF.

The delivered checkpoint refuses even an unloaded DLL at the conventional
executable-relative `Data/SKSE/Plugins/CommunityShaders.dll`, so ordinary delayed
SKSE loading cannot silently evade prepare/start's module-presence check. Both
checkpoints emit their specific refusal through the existing status/log path.
This is deliberately narrower than an arbitrary live-load guarantee: a DLL
installed after activation, redirected mod-manager paths, or an arbitrary later
LoadLibrary outside that path remains a startup-contract blocker. No coexistence
admission is added, and no late-load safety claim is made.

## Validation and next checks

The exact reference file hash, PE fields and all four guard sequences were
read directly with a bounded offline PE parser. Source/native ordering review
and scoped whitespace checks were performed. C++ was not built or executed;
no game, GPU, installation change or commit occurred. CPU Release validation
reported by the coordinator predates these compatibility files.

Coordinator should build Release `rsf_game_skyrim_se`, run the Skyrim plugin
contract and the normal native gate after integration. Future focused cases
should cover absent module/file, installed known release, unknown hash, changed
mapped entry, unreadable paths and CS appearing between prepare/start. Actual
handover acceptance additionally needs controlled loader ordering, desired
settings save/reload preservation, HDR fallback coverage and a moving game
session with other CS features retained. None has been claimed here.

## Exported-initializer interception continuation

The previous requirement for a new generic startup ABI was too broad. The
existing adapter can intercept the named initializer lookup and return a wrapper
that executes synchronously before the original initializer. No carrier change,
SKSE header/library, setting or new framework is required. This continuation
implements that narrower mechanism; it does not claim game-tested coexistence.

After the executable and native-byte check, the adapter arms a process-lived
detour of the public GetProcAddress API while CS is absent. It preserves the
original API result and LastError for other lookups. Only a named
`SKSEPlugin_Load` lookup on the actual `CommunityShaders.dll` module returns
the adapter initializer. The resolver performs no file hashing, patching,
logging, waiting or SDK work. The supported SKSE reference sequence is
LoadLibrary, then GetProcAddress, then synchronous initializer invocation.
This uses the public API boundary, not guessed loader RVAs or a new private
SKSE binary profile. The current reference version header is 2.3.1.

The initializer wrapper refuses a loader callout before file inspection or
MinHook work. It resolves `RtlIsThreadWithinLoaderCallout` dynamically and
refuses if unavailable or true. The native function's signature is recorded
by the primary [System Informer phnt header](https://github.com/winsiderss/systeminformer/blob/master/phnt/include/ntrtl.h).
This observation is a guard for the supported synchronous initializer path,
not a general proof for arbitrary code that manually holds internal locks.
[Microsoft's loader guidance](https://learn.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-best-practices)
explains why patching and thread synchronization must stay outside DllMain.
The [GetProcAddress contract](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-getprocaddress)
provides the exported-address boundary; it does not itself execute that export.

Before forwarding the initializer, the wrapper requires all of:

- CS was absent before interception; a module appearing before arming completes
  without an intercepted lookup poisons ownership instead of guessing freshness.
- The file hash, PE identity and five mapped entry guards match the release,
  including initializer RVA `631800` bytes
  `48 89 5c 24 08 55 56 57 41 54 41 55 41 56 41 57 48 8d ac`.
  The intercepted function address must equal module base plus `631800`.
- Upscaling.loaded is false, Streamline enabled/initialized/tried bytes at
  `fd9160/61/62` are zero, and the game has not created its renderer device
  (`3330188` owner, device `+56`), using the native-creation guards already mapped.
- The initializer is entered exactly once. Concurrent or repeated initializer
  entry, an unknown profile or a failed hook refuses this process's ownership.

The wrapper pins the exact CS module, installs the guarded void
`Feature::Load(Feature* RCX, json* RDX)` detour at `3a4b30`, then calls the
original initializer without holding an adapter mutex. The detour forwards
original configuration loading for every feature and uses an unwind guard to
clear only Upscaling object's loaded byte on return or C++ exception. Thus
configuration parsing, validation, desired settings and other feature loading
remain CS-owned. The later loaded-feature Load/PostPostLoad dispatch excludes
Upscaling before it can install its D3D/SDK/jitter/postprocess wrappers.
The exclusion also remains installed for subsequent configuration loads.
No adapter vendor SDK call, pacing choice or sleep is involved.

The post-initializer check verifies exclusion and dormant Streamline again.
Failure after original initialization does not change an original success into
failure: doing so could make SKSE discard a plugin that already installed core
hooks. RSF resource processing refuses instead. The adapter and CS hook/module
lifetimes are process-lived; stopping RSF does not attempt a late CS SDK startup
or restore the loaded flag. Restart is required to return ownership to CS.
An RSF start failure after exclusion can leave native rendering with CS's other
features and Upscaling excluded for that process; desired settings are retained.

### Fallback ownership and admission limits

The core fallback D3D wrapper changes the requested feature level and, when
HDR Display is loaded, the swap-chain format. The existing native-creation
trampoline follows the current game IAT and waits until that outer wrapper
returns before replacing the caller's native-chain reference. It does not
resolve an unwrapped D3D implementation or bypass CS's wrapper.

HDR fallback is deliberately not admitted. Native HDR thunk `45cbd0` redirects
the framebuffer, calls original through slot `13fec40` at `45cc07` (return
`45cc0d`), then restores the framebuffer. It forwards five arguments, including
the stack bool. This is not the vanilla caller `153adec` recognized by the
current SR hook. HDR object's address is `13dcda0`; its loaded byte is `+8`.
With that feature loaded, RSF skips native-chain facade admission, resolution
processing, SR/FG-input production and presentation binding, while forwarding
native execution and leaving CS HDR enabled. Status/log reports this specific
resource-ownership refusal. No HDR settings are changed to obtain admission.

CS without HDR Display can pass the new factual admission gate only after the
exact initializer completed with Upscaling excluded and Streamline dormant.
This is implemented ownership gating, not demonstrated image or performance
acceptance. Other features' resource/shader compatibility still needs game tests.

Named later loads are intercepted too; an initializer after device creation is
refused before original CS startup. Direct manual export resolution, ordinal
lookups or an already initialized CS are not supported takeover paths. A CS
module without the excluded ownership state never admits RSF resource processing.
This mechanism does not arbitrate arbitrary custom loaders that bypass the
intercepted API. The official matching SKSE download currently points to Nexus,
which returned HTTP 403; no unofficial binary was acquired or loader RVAs used.

### Focused checks for this continuation

The ignored receipt `cs-initializer-boundary-20261115.json` verifies all five
C++ guard arrays directly against the exact release, initialized/tried RIP
targets, HDR object target and HDR original-call slot. Native evidence was
inspected in IDA session `b1452677` before the Orca restart. Scoped whitespace
checks passed. No C++ build, test execution, game/GPU run, installation change
or commit was performed; the coordinator's earlier native gate predates this edit.

Next validation is the focused Release build and Skyrim contracts, followed by
a controlled initializer fixture/run covering fresh load, preexisting load,
wrong hash, loader-callout/reentry refusal, configuration save/reload and
exception exclusion, native D3D wrapper preservation and HDR refusal. Live
acceptance must verify other CS features, CS SDK absence and RSF resource paths.
Full CS HDR coexistence remains unfinished and requires an explicit five-argument
wrapper/resource contract, not relaxing the vanilla caller guard.

## Five-argument chain and exact HDR blockers

Static continuation during the user's capture changed only compatibility.cpp/h,
plugin.cpp and this note. No running game was queried, modified or controlled.
Evidence in `cs-hdr-chain-20261115.json` retains matched native ProcessMain,
its complete disassembly, HDR redirect and HDR wrapper decompilation. IDA
sessions were `480c7d05` (game) and `52fb5ea5` (CS).

Native `14edbf0` consumes RCX, EDX and R8D only. Incoming R9 is not consumed;
it is overwritten for outgoing calls. Its stack home slots save registers and
R8D, with no incoming fifth-argument read. The CS wrapper nevertheless loads
the incoming fifth byte at `45cbf0`, writes it to the outgoing stack at
`45cbfe`, and calls through `13fec40` at `45cc07`, returning at `45cc0d`.
The adapter now carries five machine arguments, using uint8_t rather than
interpreting an unspecified vanilla stack byte as C++ bool.

A guarded detour of CS HDR entry `45cbd0` forwards the complete original CS
wrapper unchanged. TLS records the exact native caller `153adec` and all five
arguments, with scoped restoration on unwind. The native observer accepts a
CS call only with that scope, matching arguments, exact return `45cc0d`, exact
`ff 15 33 20 fa 00` call bytes and original slot equal to game `14edbf0`.
Vanilla retains its original exact caller guard. CS still performs framebuffer
redirection before the observer and restoration after it; the adapter neither
duplicates nor bypasses those operations. This prepares the chain without
claiming that the following resource contracts are already supported:

- Both `render_inputs.cpp` and `presentation_inputs.cpp` currently require
  R11G11B10_FLOAT scene colour. CS HDRDisplay::DataLoaded requests the native
  float16 HDR target path, and RedirectFramebuffer writes its float16 target
  into framebuffer texture/SRV/RTV slots during ProcessMain. Admitting that
  resource requires the two adapter readers to validate R16G16B16A16_FLOAT and
  its reinsertion/auxiliary contracts. These files are outside this task's scope.
- `rsf_game_render_pass` and `rsf_d3d11_presentation_binding` have no field for
  successfully applied output colour space or a leased scene's transfer/primaries.
  `rsf_generation_setup.hdr` already exists, but generation_setup() in
  d3d11_present_bridge.cpp leaves it zero. SetColorSpace1 only forwards to the
  physical chain. R10G10B10A2_UNORM can carry SDR or PQ; texture format and
  CS's requested enableHDR flag cannot prove which was successfully applied.
  The existing binding/pass contracts need an explicit valid applied DXGI colour
  space, plus source colour interpretation, mapped by the generic core to its
  existing HDR setup. No game-name branch or new vendor setting is justified.
- The current FINAL_SCENE lease copies GetBuffer before interface drawing.
  CS's clean postprocess scene lives in hdrTexture; its PQ output is composed
  later by ApplyHDR during Present. In the deferred path CS first suppresses
  the inner Present chain, then composites, then performs actual Present.
  The early backbuffer is therefore not the current HUD-less HDR display image.
  A correctly encoded clean-scene producer and lease, or a generic colour
  conversion on the explicitly described linear HDR lease, is required before
  FG admission. Passing hdrTexture as if it were the old final surface is unsafe.

The initializer audit also changed publication and failure handling: an SRW
lock publishes the complete scalar lookup record before State::captured, with
no native calls inside the lock. Initializer entry acquires a module reference
before claiming the one-shot state; validated hook owners remain pinned.
Cleanup removes only hooks created by this attempt. Pre-device guard/hook
failure withdraws RSF's engine facts and host activity before forwarding the
original CS initializer, preserving its result and other features without
claiming unknown-profile coexistence. Late-device and loader-callout cases
still refuse original startup. Successful original initialization is never
turned into failure by a post-initializer ownership check. Start rechecks after
publishing activity to close a concurrent withdrawal race.

Source freeze: six guards and the call-site bytes match the exact DLL in an
offline check; scoped whitespace checks passed. No build, GPU fixture, deploy,
commit or game interaction ran. Coordinator's integrated Release gate and a
later separately authorized initializer/chain/configuration test remain needed.
No shader/mask convention was changed; vanilla normals.z is not treated as CS's
packed transparency mask. HDR remains refused for the concrete contracts above,
not merely because the feature exists.
