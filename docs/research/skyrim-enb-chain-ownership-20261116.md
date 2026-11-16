# Skyrim ENB chain ownership, 16 November 2026

Question: does releasing the caller's ENB swapchain reference prove that RSF can
replace its HWND chain, and which boundary preserves ENB's creation/Present work?
This is read-only static analysis plus review of coordinator-owned receipts.
No build, game/GPU run, install change or production edit was performed.

## Identity and method

Installed and archived ENB0.505 `d3d11.dll` both hash to SHA256
`bfb7ff22a962fff9a0faa6f07f5a43b96e4305cc5c9efb0eedcec8d218846677`.
Installed target: Skyrim Special Edition/d3d11.dll. Archive reference:
`.local/skyrim-compat-mods-20261115/enb/WrapperVersion/d3d11.dll`.
Image base is180000000; all addresses below are RVAs in that exact image.
Skyrim1.7.104 SHA256 remains
`846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f`.

The returned Present pointer6c690 anchors the vtable rather than a guessed
constructor signature. A read-only PE-section mapping finds its pointer in
vtable18f848; exact-file GNU objdump disassembly confirms the slots and their
callers. Full ignored receipt:
`.local/skyrim-research/enb-ownership-full-disasm-20261116.txt`.
REA/Ghidra12.1.4 imports the same SHA and corroborates these routines. Its
temporary database was closed after inspection; the original DLL was unchanged.
Pseudocode tail-call warnings were checked against actual indirect-jump bytes.
The unqualified REA open initially refused ambiguous providers; explicitly
selecting Ghidra resolved it without changing tools or installing anything.

## Concrete wrapper contract

| Site | Observed behavior |
| --- | --- |
| 5e2b0 export | Calls original creation through global442ba0 at5e375, then constructs device/context/chain wrappers before returning |
| 5e550..5e5aa | Allocates80h-byte chain wrapper, installs vtable18f848, stores raw wrapper in global441d98, calls initializer5e220 with caller chain output |
| 5e220 | Sets wrapper count+20 to1, adopts caller native pointer into+28, replaces caller output with wrapper; calls underlying GetDesc into+30; no extra chain AddRef |
| 66650, slot0 | QueryInterface tail-forwards to underlying+28 with requested IID/output unchanged; returned IUnknown is native identity, not wrapper identity |
| 66660, slot1 | Increments wrapper count+20, then tail-forwards underlying AddRef; returns its result |
| 6d560, slot2 | Decrements wrapper count, then underlying AddRef/Release/Release, net one native Release; returns final native result |
| 5b830, slot9 | GetBuffer tail-forwards unchanged to underlying+28, with no wrapper-local buffer cache |
| 6c690, slot8 | Present surrounds underlying Present with ENB work; test-only Present forwards directly |

Chain Release has **no** count-zero branch, wrapper deletion, clearing of+28 or
global441d98, context ClearState/Flush, or explicit HWND teardown. The global
wrapper and copied descriptor can therefore survive a caller Release even when
the underlying native object no longer does. The cached descriptor retains the
HWND value at wrapper+60; creation also stores the descriptor's HWND in
global441d80 at5e5e4..5e5e8. A copied handle is not an owned HWND-chain reference.
The constructor and Release do not implement a migration protocol for replacing
that HWND owner.

The device wrapper is allocated40h bytes at5e4cd and retained as raw
global441d88. Initializer5e070 adopts its native pointer at+28 and count1,
without another AddRef; its first three vtable entries are the same
66650/66660/6d560 routines. The context wrapper is allocated6ca0h bytes and
retained in global441d90; initializer5e100 adopts native context at+6c68 and
count1 at+6c80. Releasing the chain does not release either device or context.
Context Release68600 is different: count1->0 invokes ENB cleanup48780, then
the final underlying Release, and native count0 invokes4ee00. Chain Release
does not invoke this cleanup. Runtime-held device/context references thus have
a lifetime independent of its dropped chain reference.

## Backing resources and bounded negative evidence

Creation takes a temporary GetBuffer0 reference at5e41d, annotates it, and
releases it at5e4c0. The later GetBuffer0 at5e632 is also released at5e700,
after optional startup texture upload. Annotation70f70 calls SetPrivateData
(vtable+28) with48 bytes; it does not call SetPrivateDataInterface or AddRef.
Context initialization calls pipeline snapshot72450 then reference-release
72a60, so those temporary getter references are not evidence of permanent
retention. The inspected screenshot path4a340 likewise releases its GetBuffer
reference at4a52a. These paths do not demonstrate an extra chain or backbuffer
reference that a caller may consume.

ENB initialization48650 invokes4ef40, which creates persistent ENB-owned GPU
resources;4c870 releases such resources. Device/context and these resources
survive chain Release because that routine does not run their cleanup. This
does **not** identify a specific retained native backbuffer/RTV at the failing
handoff. A complete heap/refcount census was not performed. Native deferred
destruction, another owner and outstanding bindings remain possible causes of
HWND occupancy; no one of these is proven by static wrapper counts or raw
global pointers. No force-Release of an external reference is justified.

## Presentation boundary and smallest supported contract

Normal Present6c690 calls4ae30 before native Present, applies the wrapper's
interval selection, calls native+40, handles device removal/reinitialization
through48690/486d0, and calls487c0 afterward. Replacing the returned outer ENB
chain with RSF's facade bypasses those ENB Present calls, independently of
whether native-chain destruction succeeds. RTV wrapper5c170 forwards native
CreateRenderTargetView and adds ENB resource bookkeeping on success; preserving
only a device pointer is not equivalent to preserving the full wrapper chain.

The smallest safe **existing-chain/SR** boundary is to keep the returned outer
chain and its actual backbuffers/Present path, observing it without substituting
a second HWND presentation owner. This is a contract recommendation, not an
implemented or tested SR compatibility claim, and it does not supply vendor FG.

For a single-owner **FG** path, the supported ordering requirement is stronger:
the application's virtual-chain/backbuffer contract must exist before ENB
captures its inner chain, while ENB's outer device/context/Present wrappers
continue to operate on that contract. A creation interception below the outer
wrapper, before an independent real HWND chain escapes, is the candidate
boundary. The exact generic creation API and identity behavior need design and
tests; simply moving the current post-return replacement one call inward is
not proven sufficient. A late outer replacement, an ENB-global pointer rewrite,
or calling ENB Present on an already released inner chain is not supported.
Native addresses and adapter-specific wrapper interception belong in Skyrim;
runtime services must express generic creation/ownership contracts, with no ENB
name branch or guesses about external refcounts.

## Runtime evidence and remaining decision

Coordinator PID34956,
`.local/skyrim-research/input-acceptance-20261116-001939/loader.log`, records
native wrapper Present18006c690, a distinct native IUnknown, successful outer
facade admission, then **two** physical CreateSwapChain refusals80070005
(E_ACCESSDENIED) for the same HWND. The following GetBuffer returns80004005 and
null. This proves failed physical creation, not its exact outstanding owner.
The prior actual AV through ENB5c19c/game10124de is retained in
[the optional-loading note](optional-d3d11on12-loading-20261115.md); clearing the
failed output repaired one safety defect but did not establish coexistence.

Key REA evidence IDs:

- Constructor/export: `ev_36b92dad7ad4fa8b1e9f9a765b51643b9022e6a2d371d5b63e3251859c58d2c8`.
- Chain initializer: `ev_b6d313f09c660653eba0daf88d07ee855f1b61c4ff5c3cd3241f740559e4b51f`.
- QI/AddRef: `ev_a82195131000d81f30ade8137d12a878fa4dfdec092de82c763e2a08328c2b5c`, `ev_7c548b5927349a759be865236d0e9ef2b0601bb93104d53815eb17ef80392c9e`.
- Chain Release: `ev_abaf310c106f28b7932094aaed7c1abb38c8047c07a6b39a312f19cd3218c94e`.
- Context Release: `ev_5eb4e59c95dd6cf82604f3dffb1c1def4edaa79a8318bb4eca69fe0e54a7be7c`.
- Present/RTV: `ev_6e8c07d410af1cc9df6fa12525ea4b1b223fb27a7c053d78443a9c5570a682ef`, `ev_2b4a34941f2df52533054667802f09dde7f743b5b9725f6bed474fcf1b475f37`.
- Private-data annotation: `ev_1fb1a1e28b31c668a2c079a5fafd0cdeced2bcbd60971b3a374b750c28e17b59`.

Source/disassembly review and note whitespace checks are the only worker
validation. Actual final native refcount and deferred destruction remain
unmeasured; no ownership implementation or rendering acceptance is claimed.

## Scoped creation adapter wiring

The subsequent source change implements the adapter side of the agreed generic
Host ABI6 contract. This supersedes the earlier late outer replacement; it is
source-only, with no build or runtime acceptance yet. It targets full SR/FG
ownership through a single presentation owner, not an SR-only coexistence fallback.

The existing game thunk at15b9412 retains expected bytes `ff 25 c8 fe 20 00`
and the call at1012320 retains `e8 ed 70 5a 00`; its native return is1012325.
That thunk continues through the current IAT17c92e0, preserving the entire
external wrapper chain. Before calling it, the adapter verifies the caller,
owner global3330188, chain/device/context output addresses at owner+96/+56/+64,
and HWND at owner+72 against a guarded copy of the native swapchain descriptor.
It also requires active adapter and established compatibility ownership.

`presentation_bridge=3` enables generic factory interception only within the
explicit creation transaction. The adapter initializes
`rsf_d3d11_creation_scope` with size, ABI6, verified HWND and token0, then calls
`rsf_d3d11_host_creation_begin`. On successful begin a local lifetime guard
calls `rsf_d3d11_host_creation_end` exactly once after the complete native call,
including native failure/unmatched outputs. The latter cases supply null; a
successful owner/window/output recheck supplies the borrowed device, context
and outer-chain output address through `rsf_d3d11_native_chain`. End validates
the core's captured inner facade and canonical aliases; it never replaces or
releases these caller outputs. The local lifetime guard also closes a begun
transaction if native execution unwinds by C++ exception.

All original creation arguments/output pointers, the native HRESULT and its
LastError remain intact. No ENB globals, private entry points or native object
refcounts are patched. The code does not call the old post-return wrap service.
Outside the verified owner, or if begin refuses, the same native call still
runs unchanged. CS pre-initializer and HDR ownership refusals remain in force.
The generic core owns inner facade creation, canonical aliases, shared
GetBuffer surfaces and source/actual-Present binding. ENB's constructor must be
able to GetBuffer0 as ID3D11Texture2D, GetDesc, SetPrivateData with48 raw bytes,
perform optional startup texture updates and Release, before an independent
physical HWND chain exists. Its outer Present must still reach the inner
facade once, with ENB's pre/post work intact. Device/context QI and returned
identity validation remain generic COM requirements.

Coordinator verification must establish the new creation scope captured one
inner owner, the outer ENB chain stayed installed, and backbuffer resources,
actual Present, SR and FG remain matched. Failure/refusal paths must close
their token without changing native outputs. Exact native refcounts and
deferred destruction from the abandoned outer-replacement path remain
unmeasured; no successful new ENB run is implied by this wiring.
