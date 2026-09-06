# Orchestrator

This is the in-game runtime. It will own plugin selection/lifecycle, configuration, vendor backends, graphics resources, frame IDs, latency, and presentation.

Loading the DLL does not activate graphics features. The intended sequence is: establish shared services, select the game plugin, validate/prepare the game, negotiate available inputs, then activate the requested supported pipeline. Game plugins never load or own a second orchestrator.

## Frame assembly

The first thing here that is not scaffolding. A plugin knows where its engine keeps camera data and
how its velocity is stored; a backend knows what a vendor SDK wants; neither should know the other.
So a plugin fills `rsf_camera_frame`, which names nothing vendor specific, and
`rsf_assemble_dlss_frame` turns it into a backend's structure. Being the component that may know
both sides is what the orchestrator is for.

The conversion is short, and every decision in it is one this project has got wrong somewhere:

- **Which projection.** A matrix still carrying the temporal jitter makes the reconstruction correct
  for a camera that was never rendered. That reads as softness, not as a bug, so it is the mistake
  most likely to survive a look at the result.
- **What the jitter is measured in.** Pixels at render resolution, converted from the clip space the
  engine stores, dividing by the view rect rather than the buffer, because those differ the moment
  the render scale moves.
- **Whether motion has been decoded.** Assembly refuses a frame whose motion is still in the game's
  storage. Backends take a scale factor and cannot subtract a bias, so the raw target reads as a
  large constant motion across a still image.
- **What marks a pixel nothing wrote.** The sentinel the decode pass established, which is what lets
  a backend reconstruct camera motion for those pixels rather than believing them.
- **An infinite far plane.** Reversed-Z projections have none to read and backends want a number, so
  one is chosen and named rather than buried.

It also refuses rather than assembling something that will look wrong: no jitter means no extra
sub-pixel samples to reconstruct from, and an output no larger than the render size is not
upscaling.

Everything else here is still version metadata. See `docs/implementation.md` for the next work.
