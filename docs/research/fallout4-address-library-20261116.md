# Fallout 4 address-library and upstream source check

## Question and identity

The user supplied a Fallout 4 Address Library all-in-one archive and linked
[CommonLibF4](https://github.com/Ryan-rsm-McKenzie/CommonLibF4) and
[fallout4-addresslibgen](https://github.com/Ryan-rsm-McKenzie/fallout4-addresslibgen).
This check establishes the database format, resolves the pinned reference's AE
IDs, and compares actual call instructions with the installed game. It does
not install F4SE, load a plugin or change an integration offset.

The archive SHA256 is
`385ceddcf7c44d6c50cbf67eea82387f1e036447e1aabc7832ad690ce5c8bb56`.
Its12 file members cover1.10.130,138,162,163,980,984 and
1.11.137,159,169,191,221,240, each with final version component0.
The selected `F4SE/Plugins/version-1-11-240-0.bin` is10436904 bytes,
SHA256 `65985cc2259384a13cffb538d74776e422e62b0cd3766485a000620242b72b06`.
It contains652306 sorted, unique ID/RVA pairs and has exactly
`8 + 652306 * 16` bytes, with no trailing payload.
All offsets are nonzero and lie inside the installed PE's image range;
207489 land in `.text`,361999 in `.rdata` and82750 in `.data`, with68 in
the remaining mapped sections. Range membership is not semantic validation.

The executable remains Fallout4.exe1.11.240.0, SHA256
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
The database filename selects a version but does not itself embed that PE
fingerprint. Agreement below is verified separately against the executable.

## Distinct source owners

| Reference | Inspected revision | Relevant scope |
| --- | --- | --- |
| Ryan-rsm-McKenzie/CommonLibF4 | `67173f8d44b9a928dbab983d25e576ac5f1e83ac` | Original CommonLib ID database loader and single-ID relocation surface |
| Ryan-rsm-McKenzie/fallout4-addresslibgen | `4fa4032619631fa1314e1f55f8cc3eae7620f009` | Offline ID/export/diff graph and address-bin reader/writer |
| Community Shaders consumed Dear-Modding-FO4/commonlibf4 | `7f8deca16dc7dd245c9c52151ec9ef27edd9ad60` | Existing pinned OG/NG/AE IDs, layouts and render source leads |
| Consumed commonlib-shared | `5408d22adf937cc00e0ccd4bdeef66021172884d` | IDDB V0 loader and runtime variant resolution used by that reference |

The two newly supplied repositories were checked out under ignored references.
Their tracked inventories contain350 and11 files respectively. This is not a
claim of a deep audit of every upstream class. No reference code was copied,
built or executed. Their origin and revisions remain distinct from the
Community Shaders dependency and its existing full inventory.

Ryan's [IDDatabase loader](https://github.com/Ryan-rsm-McKenzie/CommonLibF4/blob/67173f8d44b9a928dbab983d25e576ac5f1e83ac/CommonLibF4/include/REL/Relocation.h#L473)
maps a leading uint64 count followed by uint64 ID/offset pairs, selecting a
version-named file from the executable version. The pinned fork's `load_v0`
uses the same representation. The supplied member matches that representation.
Ryan's version constants at this revision end at1.10.163; that is not evidence
that every original layout is valid for AE. The pinned fork's explicit runtime
field offsets and VariantID interpretation remain necessary source evidence.

The generator's [reader and writer](https://github.com/Ryan-rsm-McKenzie/fallout4-addresslibgen/blob/4fa4032619631fa1314e1f55f8cc3eae7620f009/src/addrlib.rs)
confirm little-endian uint64 count and pairs. Its [entry point](https://github.com/Ryan-rsm-McKenzie/fallout4-addresslibgen/blob/4fa4032619631fa1314e1f55f8cc3eae7620f009/src/main.rs)
reads exported offsets, cross-version diff edges and existing ID bins, seeds
the graph, assigns IDs and writes missing bins. It requires those inputs;
it is not an executable-only semantic reconstruction tool. An ID correspondence
does not establish calling convention, object ownership or resource lifetime.

## AE reference and instruction checks

All17 distinct named IDs in the pinned `UpscalingAnchors.h` were accounted for:
16 resolve for AE and `ForwardAlphaImpl` intentionally has AE ID0 because that
path is inlined into Forward. The12 applicable AE anchor calls all contain E8
with the expected rel32 target in the fingerprinted executable:

| Caller RVA | Target RVA | Boundary |
| --- | --- | --- |
| `21F0AE6` | `1839EC0` | World viewport selection |
| `21F0E4C` | `182D580` | World temporal update |
| `21F963D` | `21D7120` | First-person alpha geometry |
| `21FA283` | `2185420` | Pre-imagespace SR seam |
| `21F8BB5` | `221FB60` | Main composite draw |
| `21F868E` | `1839B70` | SSLR target selection |
| `21F88AE` | `183ACF0` | Pooled SSLR acquire |
| `21F8963` | `183AD60` | Pooled SSLR release |
| `10679F5` | `182D580` | Loading temporal update |
| `21F9F2F` | `1839ED0` | Dynamic-resolution update |
| `21F9F73` | `21F38B0` | Native depth prepass |
| `21FA036` | `21F9100` | Forward rendering |

This corroborates the current hook leads, including the existing guarded DR
and prepass sites. It does not establish actual runtime invocation or fix the
rejected image. No production offset needed changing for these checks.

The archive also independently confirms the [earlier cross-version correction](fallout4-native-motion-history-producers-20261116.md):
Main.UpdatePlayer ID2228929 maps to `C34C10` in1.11.221 and `C34FA0` in1.11.240.
SetSequencePosition ID2200766 maps from `4F93D0` to `4F96F0`, while temporal
update ID2277095 maps from `182D0C0` to `182D580`. Different shifts rule out
using one blanket relocation delta. The current TAA vtable ID1340364 resolves
to `2915EB0`, matching the independently checked getter slot.

## Retained evidence and limits

Ignored receipts `user-address-library-20261116.json`,
`user-address-library-selected-ids-20261116.json`,
`user-address-library-upscaling-anchors-20261116.json` and
`user-address-library-callsite-checks-20261116.json` retain the archive inventory,
member digest, selected cross-version mappings and exact call bytes/targets.
`user-address-library-image-ranges-20261116.json` retains the complete structural
range count, and `ryan-fallout4-reference-inventory-20261116.json` pins both new
reference file inventories.
The extracted database and reference checkouts remain untracked.

All results here are static source/data/instruction checks. Runtime ownership,
complete shader semantics, native TAA reset, SR/FG image quality and latency
still require their separate proofs and bounded game validation. Address-library
data used for research does not add an F4SE runtime dependency to RSF.
