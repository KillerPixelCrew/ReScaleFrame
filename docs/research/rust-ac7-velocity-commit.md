# AC7 velocity-pass commit and profile policy in Rust

Base: `2b1af9eb` on `NightHammer1000/rust-ac7-velocity-commit-policy`. This is a source comparison
of [velocity_pass.cpp](../../games/ac7/src/velocity_pass.cpp) before and after the port. Nothing
was built, linked or run for it. It continues
[the stream policy port](rust-ac7-velocity-stream.md).

## Question

Can the translucent velocity pass's commit decisions (pass admission, the commit's view identity,
the native motion target's extent, sample, array and format check), its log budgets and its
per-frame CPU profile with the report thresholds move into `rsf-ac7`? The native depth identity,
the target's description and device check, the state scope, the merge draw, the copies, the clock,
the HLSL and the public C entry points stay as they are.

## Traced paths at the base

| Path | Behavior at `2b1af9eb` |
| --- | --- |
| Commit admission | `rsf_ac7_velocity_commit` returns 0 without a renderer, context or pass, with `struct_size < sizeof(rsf_game_render_pass)`, or without `motion` or `depth`. It builds `{session_id, history_key, view_key, family_key, uint32_t(native_frame), 0, 0, 0}` and asks `rsf_ac7_velocity_view_commit`, which needs a written view at that frame, view key and family key. A non-commit action or a slot outside the native views returns 0. |
| Native depth | The view's stored native depth must be `pass->depth`. |
| Target check | `GetDesc` on `pass->motion`: Width and Height must equal the view's extent, `SampleDesc.Count` and `ArraySize` 1, Format `DXGI_FORMAT_R16G16_UNORM` (35) or `DXGI_FORMAT_R16G16_TYPELESS` (33). Then the context's device must be the renderer's. |
| Completion | After the merge and both copies, `rsf_ac7_velocity_view_committed` clears `written`. With a log attached, `commit_reports++ < 3` logs the commit line. |
| Refusals | With a log attached, `refusal_reports++ < 12` logs one refusal. `rsf_ac7_velocity_set_log` resets both counters with every attachment, including a null one. |
| Profile sample | After renderer creation, `CpuSample` records `uint32_t(current.native_frame)` as the profile frame. Its destructor adds the callback's elapsed time, the variant admission time and the resource time, raises the peak to the elapsed time, counts the callback and raises the largest record bound. |
| Frame end | Without a renderer nothing happens. With a log attached and a total of at least 8 ms or a peak of at least 4 ms, the CPU line is logged with the frame, callbacks, the four times, the largest records, `rsf_ac7_velocity_model_count`, `rsf_ac7_velocity_variant_count` and the device's shader object count. The four times, callbacks and largest records are then zeroed either way; the frame is kept. History retirement follows. |
| Renderer replacement and release | A new `Renderer` starts every profile field at zero. Release destroys it. Neither touches the log counters. |

## Moved

[velocity_commit.rs](../../games/ac7/src/velocity_commit.rs) now owns, through
[velocity_commit.h](../../games/ac7/src/velocity_commit.h):

- `rsf_ac7_velocity_commit_view`: the pass's null and size check, both leases, the commit identity
  from the pass's exact session, history, view and family keys and the low 32 bits of its native
  frame, and the view lookup through `velocity_history::commit`. The decision is unavailable on any
  refusal. `rsf_ac7_velocity_view_commit` is removed; nothing else called it.
- `rsf_ac7_velocity_commit_target`: the extent, sample, array and format check against the
  committed view, from the five native description fields.
- `rsf_ac7_velocity_report` and `rsf_ac7_velocity_reports_reset`: the 12 refusal and 3 commit
  reports per log attachment.
- `rsf_ac7_velocity_profile_sample`, `rsf_ac7_velocity_profile_frame_end` and
  `rsf_ac7_velocity_profile_reset`: the accumulation, the 8 ms total and 4 ms peak thresholds, the
  restart at every frame end that keeps the frame, and the reset on renderer replacement and
  release.

`velocity_pass.cpp` keeps the renderer and context checks, the native view slot bound and depth
identity, `GetDesc`, the device check, the state scope, the merge draw, the copies,
`rsf_ac7_velocity_view_committed`, the QPC clock and the sample's phase timing, the shader object
count and every log text and its formatting. The model and variant counts are still read when the
line is formatted, before history retirement. The public calls in `ac7_velocity_pass.h` are
unchanged.

## Deliberate differences

- The pass checks run after the renderer and context checks instead of interleaved with them.
  All are pure reads, so the outcome is the same.
- A spent log budget no longer counts on. The old counters kept incrementing and would have logged
  again after wrapping past `UINT32_MAX` refusals or commits.
- The log budgets and profile share one lock. The old counters were unsynchronized globals; the
  pass's calls are single-threaded and the lock is defensive.
- The profile frame is captured when the sample is constructed and handed over with it, instead of
  written into the renderer at construction. `current` does not change during a draw callback, so
  the reported frame is the same.

## Checks

Source comparison against `2b1af9eb` of every former use of `refusal_reports`, `commit_reports`,
`cpu_ms`, `shader_cpu_ms`, `resource_cpu_ms`, `peak_cpu_ms`, `profile_frame`,
`profile_callbacks`, `largest_records`, `VelocityViewDraw` at the commit and the commit's
description checks. `cargo fmt -p rsf-ac7 --check`, locked offline Cargo metadata and Git
whitespace passed. The installed rustfmt is 1.100.0; the `rust-toolchain.toml` pin (1.94.1) is
not installed on this machine, so formatting matches CI only as far as the two agree. The Rust
tests in `velocity_commit/tests.rs` cover the commit identity and frame truncation, both leases,
every target refusal and both accepted formats, both budgets and their independence, the total and
peak thresholds, the restart that keeps the frame, and null and short inputs. They were not run.
Build, link, Cargo tests, Clippy, CTest including `ac7_velocity_shader`, deployment and game runs
are user-owned and not done.

## Remaining in C++

The device renderer lifecycle and variant construction, every D3D11 call and description, the
native resource identity checks, the clock and log text, and the HLSL.
