# Legacy Streamline SR resolution in Rust

Compared with `a5867f2f`, the Rust session owner now defines the required
interposer and DLSS feature lookups. C++ binds the resolved addresses to the
vendor header types and calls Streamline with its original signatures. The
owned route resolves against the already loaded, optionally signature-checked
module before `slInit`. The borrowed route uses the D3D12 host's module after
checking its graphics device, without acquiring a loader reference or
initializing the host again.

The required chain starts with `slIsFeatureSupported`, `slGetFeatureFunction`,
`slGetFeatureRequirements`, `slSetConstants`, `slSetTagForFrame`,
`slEvaluateFeature` and `slFreeResources`. The owner continues with `slInit`,
`slShutdown`, `slSetD3DDevice`, `slGetNewFrameToken` and `slAllocateResources`.
The first missing address uses the existing diagnostic and stops the chain.
The private address array is now staged before binding. Previously a failed
chain left partial typed entries in the fresh `Native` object until it was
destroyed. Destruction clears the same log registration in both versions.

Both feature routes look up `slDLSSGetOptimalSettings` before
`slDLSSSetOptions` through the typed `slGetFeatureFunction` callback for
`sl::kFeatureDLSS`. Borrowing stops on the first SDK error. A null address
from a successful first lookup still allows the second lookup. Both routes
return `MISSING_ENTRY_POINT` for an incomplete pair. After a successful
owned `slSetD3DDevice`, the probe tries both lookups and derives the
capabilities from their addresses, including any address returned alongside
an SDK error. It publishes available functions even when the pair is
incomplete. The C++ lookup now catches a thrown SDK
exception at the callback boundary. The caller sees the previous result
(`INIT_FAILED` for borrowing, `FEATURE_FAILED` for the device probe), and
neither feature address is rebound on that path.

Source comparison matched each of the 12 address slots with its original
typed member, both feature slots, and all 16 slots and bindings in the
existing C++ and Rust platform tables. The two private resolver symbols are
referenced only by the native bridge and its Rust static library. The 12 C
wrappers and declarations, SDK-disabled stubs, signature checks, SDK
preferences, resource tags and host platform sources matched the baseline.
Pinned Rust formatting, locked offline Cargo metadata and whitespace checks
passed. Compile and link for both SDK configurations, PE export comparison,
Clippy, tests, and device and game verification remain pending.

## Owned and shared SR startup

Compared with `d4d7504c`, Rust now holds the native capsule during
owned startup and shared-host borrowing. Owned startup establishes
the log sink, resolves the twelve entries, and calls the typed
initializer. The initializer still adds manual hooking and
frame-based tagging to the SDK defaults, loads DLSS, and retains
the optional plugin paths. Application and engine identity, log
level and D3D11 remain at the existing values.

Borrowing still checks the existing host module and native device,
resolves the seven shared entries and strictly checks the feature
pair before publishing the borrowed route. Owned device registration
still calls `slSetD3DDevice` before resolving the pair. Rust retains
the distinction between a shared lookup error and an owned lookup
that returns a usable address alongside an error. Failed startup
disposes of the capsule and clears its log registration.

The private startup table has 12 callbacks and the main table 14.
Source comparison matched their layouts, the pinned manual-hooking
and tagging preferences, the public signatures and disabled stubs.
The added fixture covers the strict and owned feature distinction.
Pinned formatting, locked offline metadata and whitespace passed.
Builds, fixtures and Standard and AVX live verification remain open.
