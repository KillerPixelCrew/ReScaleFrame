# Dependencies and references

All first-party code lives in this repository. The initial native scaffold depends only on the Windows/C++ toolchain. The egui dependency is pinned in the Cargo workspace, with transitive versions recorded in `Cargo.lock`.

No vendor graphics SDK, reference repository, game binary, or Epic engine source is copied into this repository. The research source map records inspected revisions and upstream links. Epic links require authorized access.

Vendor backends will need separately managed SDK/runtime dependencies and their applicable notices. Before incorporating reference code, check that project's actual license and compatibility with ReScaleFrame's selected license. Studying a source file does not make it first-party code.

First-party runtime, loader, launcher, UI, game plugins, tools, and documentation use GPL-3.0-only. The Game SDK under `sdk/game/` uses MIT. No third-party runtime binaries are currently distributed.
