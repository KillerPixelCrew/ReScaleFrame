# Naming Unreal functions from reflection data

Inspection: 7 September 2026, against the decrypted AC7 module dump described in
[binary analysis](ghidra-tooling.md) and the Ghidra project `AC7Dump` (image base `0x140000000`,
161,554 functions). Engine source is the pinned `4.18.3-release` checkout, `0a14a8d537a3`.

## The question

The dump has 161,554 functions and almost no names. AC7 ships no symbols, its RTTI yields only
twenty middleware descriptors, and Function ID databases need a reference build that nobody has
compiled. The question was whether a reflection SDK dump, which carries names but no addresses,
can be joined to the binary, which carries addresses but no names.

## Why this approach

`references/ac7-sdk` (`680e6dd`, from `Gteditor99/ace-combat7-SDK`, game version 1.3.0) is
generated from the game's own reflection data: 2,376 classes, 1,470 structs, 22,981 fields with
exact offsets, and 6,051 reflected functions. It has no addresses at all.

The join exists because Unreal's generated code has to hand the same names to the engine at
startup. Two static arrays do it, and both survive into a shipped binary:

```cpp
struct FNameNativePtrPair     { const char* NameUTF8; Native Pointer; };            // CoreNative.h:19
struct FClassFunctionLinkInfo { UFunction* (*CreateFuncPtr)(); const char* FuncNameUTF8; };  // Class.h:1998
```

`Funcs[]` holds a class's native functions against their exec thunks. `FuncInfo[]` holds every
function of the class, native or not, against its `Z_Construct_UFunction_<Class>_<Name>` singleton.
The generated name is built in
[CodeGenerator.cpp:813](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Programs/UnrealHeaderTool/Private/CodeGenerator.cpp#L813).

This costs no engine build and does not depend on compiler or optimisation settings, which is what
makes it different from signature matching.

## The trap

The two structures hold the same two pointer kinds in opposite order. On x64 both are sixteen byte
entries alternating a name pointer and a code pointer, so scanning for one shape finds the other
**shifted by one word**, pairing every name with the *next* entry's pointer. A first pass that
assumed a single shape reported 8,157 matches across 487 classes and looked entirely plausible;
roughly half of them were off-by-one nonsense.

The fix is to not assume the phase. Every aligned word of the data sections is classified as a name
pointer, a code pointer or neither; maximal alternating spans are taken; and the shape is read from
which kind the span begins with. A span that begins with a name is `Funcs[]`, one that begins with a
constructor is `FuncInfo[]`.

Two further corrections were needed:

- Consecutive arrays of the same shape run together, because the alternation does not break at the
  boundary. The engine emits each array sorted case-insensitively by name, so a name that does not
  advance marks a boundary. Where the ordering hides one (the second array's names happen to sort
  after the first's), the span is split by taking the class that explains the longest run from the
  current position.
- One address can carry several names. These are not contradictions: the linker folds identical
  bodies, so `AAIGameObject::execGetLockedOnTarget` and `AGameObject::execGetLockedOnTarget` are one
  function, and 24 trivial constructors share a single address. 166 addresses are folded. They are
  named once and the other names recorded, not discarded.

## Result

| Recovered | Count |
| --- | --- |
| exec thunks | 4,676 in 325 classes |
| `Z_Construct_UFunction_*` | 6,052 in 390 classes |
| distinct named addresses | 10,728 |
| arrays whose size equalled the class's declared count | 4,292 exec, 4,486 constructor |
| rejected | 8,897, all single entries below the two-entry threshold |

`AActor` resolves to exactly 103 exec thunks against 103 declared natives, and exactly 125
constructors against 125 declared functions. Every accepted array being a whole-class match is the
scan's own consistency check: a wrong phase or a missed boundary does not produce one.

## Evidence

Three addresses were decompiled blind and checked against what the name predicts.

| Address | Name | What the code does |
| --- | --- | --- |
| `0x141b24d40` | `AActor::execWasRecentlyRendered` | unpacks one float, calls `0x141571e80(this, Tolerance)`, stores a bool |
| `0x141b9a280` | `UKismetMathLibrary::execBooleanXOR` | unpacks two bools, stores `a != b` |
| `0x141b20400` | `Z_Construct_UFunction_AActor_ReceiveEndPlay` | `if (!Singleton) Construct(&Singleton, ...); return Singleton;` |

`execBooleanXOR` is the strongest of the three: nothing about the scan knows what XOR means, and the
body computes it.

The argument unpacking also identifies its own helpers. `FFrame::StepCompiledIn` branches on `Code`
and otherwise walks `PropertyChainForCompiledIn`, which fixes both callees:

| Address | Name | Evidence |
| --- | --- | --- |
| `0x140bcf370` | `FFrame::Step` | taken when `Code != 0`, called as `(this, Object, Result)` |
| `0x140bcf3a0` | `FFrame::StepExplicitProperty` | taken when `Code == 0`, called as `(this, Result, Property)` |
| `0x141571e80` | `AActor::WasRecentlyRendered` | the call the exec thunk makes before storing its result |

Field offsets in those bodies match `FFrame` in
[Stack.h](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/CoreUObject/Public/UObject/Stack.h):
`Object` at `0x18`, `Code` at `0x20`, `PropertyChainForCompiledIn` at `0x80`, and `UField::Next` at
`0x28`. Those five names are applied in `AC7Dump`; the remaining 10,728 are not yet.

## What this does not establish

An exec thunk is a generated argument unpacker. Naming it does not name the engine function it
calls, though that function is one call away and the thunk's shape makes it easy to reach; that pass
is not written yet.

Nothing here touches the renderer. Reflection covers `UObject`-derived gameplay and UMG types, so
`FSceneRenderer`, `FPostProcessing` and the RHI stay unnamed. Those need the source-string anchors
in [source-map.md](source-map.md) and `tools/map-source-files.py`, which is a separate route.

The SDK is a dump of game version 1.3.0. A layout or function set from it is evidence about that
build. Where the scan found an array the SDK does not fully account for, the array was split rather
than forced, and the residue is reported rather than named.

## Tools

| Tool | Role |
| --- | --- |
| `tools/parse-ue-sdk.py` | generated SDK headers to a JSON index of classes, structs, enums and functions |
| `tools/find-native-registrations.py` | scans a dump for both array shapes and matches them against the index |
| `tools/ghidra/apply-native-names.py` | applies the result to a Ghidra program, dry run by default |

The first two run off the dump and the SDK alone. Applying needs Ghidra: the MCP bridge refuses
script execution unless `GHIDRA_MCP_ALLOW_SCRIPTS=1` is set, so the applier is run from the Script
Manager of the open project, or headless when the project is not locked.
