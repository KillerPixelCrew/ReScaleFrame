# AC7 view uniforms owned by Rust

The existing view reader and jitter removal moved from `view_uniforms.cpp` to `view.rs`. The
same [captured layout](ac7-frame-capture.md) provides camera position, basis, projection,
dimensions and current and previous jitter. The port preserves the row-major layout, pixel
conversion, perspective test, correction order and previous-frame fields. It reuses the
GameSDK's f64 matrix inverse with the same pivot floor. Bindgen generated the Rust record
from the existing public header.

The original projection and basis comparisons used `abs(value) > tolerance`. A NaN made
that comparison false and could pass admission. The Rust reader rejects nonfinite input
and derived values in those checks and publishes the output only after a successful read.
Byte-wise float decoding also admits an input buffer without float alignment. Jitter removal
keeps the copy-on-refusal behavior and lets the input and output alias.

Both DirectInput carriers and the game DLL already consume `ReScaleFrame::AC7View`. That
target now carries the Rust archive with the former C symbols. The public header and carriers'
export definitions are unchanged. The two carriers now link this archive alongside the
orchestrator Rust archive. The combined linkage still needs a native build.

Source checks matched the 19 view offsets, ABI record and public entry names. Pinned formatting,
locked Cargo metadata and Git diff checks passed. The existing C++ fixture now includes a NaN
inverse and an unaligned buffer. Builds, fixtures, captured-buffer replay and gameplay remain
unrun for this port.
