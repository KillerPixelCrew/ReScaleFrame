# In-game overlay

The overlay panel: what the runtime told it, what it drew, and what the user asked for. egui in
Rust, behind the C ABI in [`include/rescaleframe/overlay.h`](include/rescaleframe/overlay.h).

This crate never touches a GPU. It produces triangles and a texture atlas; a D3D11 renderer in the
orchestrator draws them inside the game's own frame.

## Why the split falls here

An overlay drawn inside somebody else's frame has to leave the pipeline exactly as it found it.
That is C++ work against a device the game owns, on the game's render thread, with every piece of
state that has to be captured and restored. Deciding what a quality dropdown does is not that work,
and doing it in C++ next to the device would mean testing widget behaviour by launching a game.

So the line is triangles. Above it: widget state, layout, the decision about what a click means, and
the wording on the panel, in safe Rust that a `cargo test` can drive at full speed with no device,
no swap chain and no game. Below it: buffers, scissor rectangles, texture uploads and pipeline
state. The header in `include/` is the whole of the agreement, and it is a C ABI because that is the
only thing both sides can compile.

The crate is two layers for the same reason, one step further in.

| Layer | Files | What it is |
| --- | --- | --- |
| Safe core | `model.rs`, `panel.rs`, `overlay.rs` | Ordinary Rust. Takes a `FrameInput` and a `Stats`, returns an `Intent`, and keeps the vertices, indices, draw calls and texture patches. Every unit test lives here. |
| C boundary | `abi.rs`, `ffi.rs` | The header's structs mirrored field for field, and the five entry points. Checks pointers, checks struct sizes, catches panics, copies. No layout decisions. |

`abi.rs` is the only file that repeats the header, so a change to the header is a diff in one place.
Its layout test pins the offsets a C compiler produces on a 64-bit target.

## What the panel shows

An enable toggle and a selector over the five quality levels, both of which produce intent. The
render and output sizes and the ratio between them, since that ratio is what a quality level
actually means, computed from the two sizes rather than from the level. Which inputs were found this
frame, in words and colour: scene colour, depth, motion, exposure. Whether the motion has been
decoded and whether the projection carries a jitter, both of which have been assumed wrongly in this
project before and so get their own rows. The counters, presented, evaluated and refused, with the
backend's own last result code once something has been refused. The backend name, and the refusal
reason when it is unusable.

The quality selector is greyed out when no backend is loaded, with the reason next to it, rather
than letting someone pick a level that will do nothing.

There is no frame rate gain, no latency figure and no quality score, because the project cannot
measure any of the three. A presentation counter is not a latency measurement and a status panel is
the last place to imply otherwise.

## What a renderer needs to know

Beyond what the header says:

* **Colour** is premultiplied RGBA packed with red in the low byte, so the bytes in memory read R,
  G, B, A. That is `DXGI_FORMAT_R8G8B8A8_UNORM`.
* **Indices are call-local.** Add the draw call's `vertex_offset`, or pass it as
  `BaseVertexLocation`.
* **Positions and clip rectangles are in physical pixels** already. The panel scales itself with the
  display height, so a 4K display gets a 2x panel.
* **Empty means null.** With nothing to draw, the pointers in `rsf_overlay_draw_data` are null and
  the counts are zero.
* **One thread.** The handle carries no lock of its own and every entry point takes it exclusively,
  so all five calls have to come from the same thread, the one that renders. A window procedure
  that sees the mouse belongs on the other side of `rsf_overlay_input`: hand its latest position and
  button mask to the render thread and pass them in, rather than calling in from the message thread.
* **Set `struct_size` on all four structs**, the two outputs included. A struct shorter than this
  build's version of it is refused with `RSF_OVERLAY_ERROR_INVALID_ARGUMENT`; a longer one is fine
  and means the caller has a newer header.
* **The first frame draws nothing.** egui lays a new window out once invisibly to learn its size, so
  the first call after creation returns no draw calls. The second one draws.
* **Collect textures after every frame,** and loop until the call returns zero. Entries are handed
  out once each. The font atlas arrives as one whole-texture update and then as partial patches for
  as long as new glyphs keep appearing, so a renderer that only handles whole updates looks correct
  until the first time it is not.
* **A `delta_seconds` of zero is read as one sixtieth.** The header says zero is survivable, and it
  is, but egui drives its appearance animation from elapsed time, so a clock that never advances
  leaves the panel permanently part way through appearing. Send a real frame time if you have one.
* **The atlas is at most 2048 pixels a side.** Chosen conservatively rather than measured: this
  crate never sees the device and cannot ask.
* **After `RSF_OVERLAY_ERROR_PANICKED`, only destroy.** A panic caught mid-egui leaves its context
  locked, so the handle refuses everything else rather than deadlocking the render thread. Panics
  are caught because unwinding into a game's frame is undefined behaviour; that only works while
  panics unwind, and a profile built with `panic = "abort"` would take the game down instead.
* **Draw nothing for a frame whose call did not return `RSF_OVERLAY_OK`.** `rsf_overlay_draw_data`
  is filled in only on success. An argument that was refused never reached the layout and leaves the
  previous frame's arrays alone, but a caught panic happened part way through one, and by then the
  vertices, indices and draw calls those pointers name have already been thrown away. Since the
  return code does not say which, treat any failure as nothing to draw this frame.

## Build and test

```bash
cargo test -p rescaleframe-overlay
cargo clippy -p rescaleframe-overlay --all-targets -- -D warnings
cargo fmt
```

The tests drive the whole panel through `Overlay`: they click the controls at the rectangles the
panel reports, so a layout change cannot quietly turn a click into a click on nothing. A smaller set
in `ffi.rs` covers the boundary's own rules, the version gate, the pointer and struct size checks,
and one frame walked the way a renderer would walk it.

## Not verified

Everything below is unproven, not merely untested.

* **Never run in a game, and never drawn.** Nothing here has been through the D3D11 renderer or
  injected into a process. That the triangles are correct is an assertion about buffer indices and
  scissor bounds, not a picture anyone has seen.
* **Never built with MSVC.** It compiles and tests on Linux, and cross-compiles to a Windows DLL
  with the `x86_64-pc-windows-gnu` target, exporting the five functions the header declares. That is
  not the MSVC build CI runs.
* **No C or C++ has ever called these symbols.** The entry points are exercised from Rust only.
  The layouts have been compared against a C compiler once, by hand and outside the build: `gcc`
  and `x86_64-w64-mingw32-gcc` were pointed at the header and every size and offset they report
  matches the numbers pinned in `abi.rs`, as do the constants. That check is not committed and MSVC
  has not been asked, so the guard that survives is still the hand-written one in `abi.rs`.
* **The panic path has never fired.** `catch_unwind` is in place at every entry point and the
  poisoned handle is enforced, but no test provokes a real panic through the FFI.
* **The display scale is a guess.** One point per pixel up to 1080 lines, scaling to a hard stop at
  3x. Nobody has looked at the panel on a real display at speed.
* **Mouse handling is reconstructed, not observed.** The header hands over a position and a button
  mask, so presses and releases are recovered by comparing against the previous frame. That has been
  tested against synthetic input only, and the scroll wheel has not been exercised at all. A button
  pressed while the panel was up and released while it was hidden is released for egui on the frame
  the panel comes back, at wherever the pointer is by then; if the pointer never moved, that lands
  as a click on whatever was under it.
* **No keyboard input.** ABI version 1 carries none, so the panel cannot be typed into.
