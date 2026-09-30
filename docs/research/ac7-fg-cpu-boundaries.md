# AC7 frame-generation CPU boundaries

30 September 2026. Static source/binary inspection only. None of these sites is installed as an
FG/latency hook, and no frame handoff or latency measurement is established by this note.

## Question and method

Where can the plugin begin a source frame before input and identify simulation independently of
the renderer's Present counter? The stock outer loop is the right ownership lead: it pumps Windows
messages, polls game devices, then ticks the engine. A renderer-only marker cannot reconstruct this
CPU identity later.

Used the retained decrypted `Ace7Game.exe.dump` and its import metadata, the known installed
executable SHA-256 `c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`,
and authorized UE `4.18.3-release` source `0a14a8d537a31ecc77488ced41dbaa0166612ef8`.
The reference checkout's revision was confirmed. The configured Ghidra bridge at port 8089 was
offline, so local Capstone disassembly and PE exception ranges supplied the initial inspection.
No Ghidra function was renamed and no patch was applied. Runtime validation remains mandatory.

`tools/find-string-refs.py` located references to `t.IdleWhenNotForeground`,
`r.OneFrameThreadLag` and `Frame%d`. A raw displacement match was treated as a lead, then verified
as a decoded RIP-relative instruction. Registration functions were excluded: finding a cvar name
does not identify its caller's frame boundary. `SlateInput` and the exact proposed render-thread
timeout string were absent, so they were not usable anchors.

## Static results

The primary candidate for `FEngineLoop::Tick` starts at RVA `0x3a3f70`, with its first exception
range ending at `0x3a4863`. Its initial bytes are
`48 8b c4 48 89 58 10 48 89 70 18 55 57 41 54 41 56 41 57`.
That range hashes to SHA-256
`321f29242301dde4e9b46444b5f970c63387231b39b9c421ab59655bb9b4897b`.
The match is supported by several independent sequences: heartbeat/tickable/movie-player work,
a call into the render-frame command containing `Frame%d`, the message pump, idle gating,
device polling/finished-input calls, media pre/post work and an engine virtual tick receiving
delta time and the idle flag. This is stronger than the cvar anchor alone.

| Site | Decoded evidence | Source interpretation |
| --- | --- | --- |
| `0x3a42c4` | `e8 f7 4a 6e 00` calls `0xa88dc0` with `cl=1` | PumpMessages before device polling |
| `0x3a42cc` | `e8 7f b5 00 00` calls `0x3af850` | ShouldUseIdleMode after messages |
| `0x3a4365` | `e8 c6 5d 94 00` calls `0xcea130` | Slate PollGameDeviceState |
| `0x3a436d` | `e8 7e 13 93 00` calls `0xcd56f0` | FinishedInputThisFrame |
| `0x3a43cd` | `ff 92 68 02 00 00`; RCX engine, XMM1 delta, R8 idle | Engine Tick virtual slot `0x268`, after input/media pre-tick |

The message-pump branch at `0xa88e06` independently resolves its RIP-relative calls to the dump's
known USER32 slots: PeekMessageW `0x2510d98`, TranslateMessage `0x2510dc0` and DispatchMessageW
`0x2510db0`. It loops while messages exist. Thus the call before idle/device polling is an actual
Windows input message pump, not a similarly named helper.

`0xcea130` checks an input-disabled counter at instance offset `0x1b0`, computes a delta from
two instance doubles and tail-calls the platform application's virtual slot `0x10`. Its early
return is `0xcea15e`. That agrees with the stock PollGameDeviceState owner and its disabled-input
condition. It is a leaf without a matching `.pdata` range; a fixed 128-byte inspection window
also contains the following function and must not be reported as its function size/hash.

The idle helper has split exception ranges. The string reference at `0x3af939` lies in its lazy
cvar-lookup tail; its primary entry is `0x3af850`. Likewise the message pump spans several ranges.
Exception entries are not universally whole function boundaries. Hook installation must respect
this distinction and use validated entry/call sites, not whichever exception fragment contains
the discovery string.

## Runtime use and remaining proof

These results inform the plugin's future outer-frame sleep and semantic marker hooks. They are
not active runtime inputs. Before installation, verify expected bytes against this executable
after decryption, calling conventions, thread identity, actual callback order, controller sampling
and loading/idle behavior. Name the sites in the matching Ghidra project when it is available.

The renderer handoff remains separate. Stock `BeginRenderingViewFamily` increments a per-scene
frame number when a scene exists and otherwise uses `GFrameNumber`; it is not a universal copy of
the outer frame counter. The queued render command must retain the plugin identity with its view
and resource generation through final UI completion and Present. A most-recent global ID cannot
substitute for that proof. No FG input or simulation marker is emitted from the legacy Present
observer as a result of this research.

Local disassembly and helper: `.local/fg-cpu-research/`. The input dump and licensed source remain
ignored. Related runtime implementation: [FG implementation](orchestrator-fg-implementation.md).
