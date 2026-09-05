# Ace Combat 7

First game target: Windows x64, DX11, customized UE4.18, Steam application 502500.

The plugin currently recognizes the researched build 9855922 by executable name, PE machine, and SHA-256. It always reports rendering unavailable. It has no hook signatures, patches, or active rendering code yet.

Next work starts with an observer for the outer frame/input boundary, TAA resources, view/frame handoff, and Canvas/Slate HUD ordering. See [the source research](../../docs/research/ue418-hook-map.md).
