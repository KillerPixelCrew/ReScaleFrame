# Community Shaders startup handover, 15 November 2026

## Scope and result

Question: can RSF automatically take SR, FG and latency ownership from the
published CS release while preserving its other features and desired settings?
The exact profile can now be validated and diagnosed, including an installed
DLL that SKSE has not loaded yet. Actual handover remains refused: the current
carrier has no synchronous pre-CS-initialization barrier. No CS private state,
settings, native instructions or vendor SDK were changed by this work.

Files changed here are `games/skyrim-se/src/compatibility.cpp`, its header,
the game CMake source list and only the prepare/start compatibility checkpoints
in `plugin.cpp`. Other existing plugin edits are preserved. CPU files are frozen.

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
