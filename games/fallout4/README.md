# Fallout 4

Native D3D11 adapter for Steam Fallout4.exe (AE), researched on 1.11.240.0. It does not use
F4SE. It owns the cold native creation and presentation binding, the CPU event sources, the
native dynamic-resolution, depth-domain and render-input readers, and the guarded SR, HUDless
and Present seams; the Runtime decides vendors, presentation and pacing. Rendering readiness
stays 0 and validation status is in [engine.json](engine.json).

The adapter finds its sites by signature, not by build (`src/discovery.rs`). Preparation scans the
loaded executable once and resolves every hooked function, callback-selecting call, patched import
slot, called function and read global: from patterns unique in its code sections with
relocations and displacements as wildcards, from RTTI class names, the import directory and the
JobListEnd job descriptor, and from operands decoded at discovered sites. The relations the old
guards fixed through displacements (a caller reaches its hook, an instruction addresses a global
or field) are checked as links. Each site is matched again in live memory, links and vtable slots
included, before its hook is created or enabled or its value trusted. A missing, ambiguous or
changed site refuses preparation, the prepare log names it, and the game keeps running natively.
The executable is recognized by name; its SHA-256 is logged and passed to the host setup only.
Structure offsets stay documented constants, fixed by the patterns that address them where code
does.

The Steam overlay forwarding the creation path relies on (`integrations/steam`) is still an exact
gameoverlayrenderer64.dll profile, because it patches the overlay's own code; another overlay
build refuses preparation with an unrecognized-fingerprint message.

Discovery has been verified against the installed executable (`discovery::tests`), not yet inside
a game run. See [site discovery](../../docs/research/fallout4-site-discovery.md) for how each site
is found, and [the render integration note](../../docs/research/fallout4-render-integration-20261116.md)
for the source and binary evidence.
