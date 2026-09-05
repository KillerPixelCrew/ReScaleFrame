# Orchestrator

This is the in-game runtime. It will own plugin selection/lifecycle, configuration, vendor backends, graphics resources, frame IDs, latency, and presentation.

Loading the DLL does not activate graphics features. The intended sequence is: establish shared services, select the game plugin, validate/prepare the game, negotiate available inputs, then activate the requested supported pipeline. Game plugins never load or own a second orchestrator.

Only version metadata exists in this scaffold. See `docs/implementation.md` for the next work.
