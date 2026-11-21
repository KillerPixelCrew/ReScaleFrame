# Fallout 4 camera math in Rust

This note covers moving `games/fallout4/src/camera.cpp` into the `rsf-fallout4` crate
(`src/camera.rs`, `src/camera/xm.rs`). It is a source port from base `9260a14`. Nothing was built,
compiled, linked or tested for it, and no game, device or window run was made. The camera
semantics it preserves are recorded in [fallout4-depth-domains-20261116.md](fallout4-depth-domains-20261116.md)
and [fallout4-native-inputs-20261116.md](fallout4-native-inputs-20261116.md).

## What moved

| Former C++ | Rust | Owns |
| --- | --- | --- |
| `camera_raster_transform` | `raster_transform` | LTRB crop admission, finite and positive native viewport, crop NDC affine |
| `camera_motion_to_uv` | `rsf_fallout4_camera_motion_to_uv` | output cleared first, negated affine scale |
| `camera_depth_domain_forward` | `depth_domain_forward` | f64 clip-box forward-W bound with one D24 step, `> 1e-6` admission, bound kept on refusal |
| `camera_composed_view_depth` | `composed_view_depth` | output cleared before inputs are read, exact forward-perspective form, both inverses required |
| `camera_frame` | `frame` | projection recovery from rotation-only V and unjittered VP, 0.0005 relative cache tolerance, jitter from the actual projection in actual raster units, 2 px jitter limit, crop composition, origin-relative world/view, reprojection and history |
| DirectXMath | `camera/xm.rs` | `XMMatrixMultiply`, `XMMatrixInverse`, `XMVector4Dot`, `XMMatrixTranslation`, load/store |

`camera.h` keeps its structures, comments and C++ signatures, including the default raster
arguments. The functions are now inline wrappers over five `rsf_fallout4_camera_*` exports, and
both sides assert the structure sizes and offsets (`NativeCamera` 288, `CameraRaster` 32,
`CameraDepthBound` 48). `render_inputs.cpp` and `depth_domains.cpp` are unchanged.
`tests/fallout4_camera.cpp` is unchanged; its target links `rsf_fallout4_rust` instead of
compiling `camera.cpp`, as the Skyrim and RDR2 camera fixtures do. The crate imports no native
symbol, so the fixture needs none of the plugin's leaves.

## Why DirectXMath is transcribed and not replaced

The question was whether the GameSDK `camera_math::inverse_4x4` could serve. It cannot without
changing behaviour. It works in f64 with partial pivoting and refuses only below a 1e-12 pivot;
the C++ refused a DirectXMath inverse whose single-precision cofactor determinant was not finite
or not above 1e-8 in magnitude, or whose result was not finite. A diagonal of `1e-3` (determinant
1e-12) is one case the SDK inverts and the camera refuses; the Rust unit test keeps it.

The C++ compiled against the Windows SDK 10.0.26100 headers, `DIRECTX_MATH_VERSION 320`. The
build passes no `/arch` option (`CMakeLists.txt`, `rsf_build_options`), so `__AVX__` and
`__AVX2__` are undefined and DirectXMath selects only `_XM_SSE_INTRINSICS_`
(`DirectXMath.h` lines 44 to 86). That path has no FMA (`XM_FNMADD_PS` is `_mm_sub_ps(c,
_mm_mul_ps(a, b))`), no SSE3 `hadd` and no SSE4 `dp` dot product. Each SSE lane is one IEEE single
operation, so `xm.rs` repeats the SSE bodies of `XMMatrixMultiply` (`(x*b0 + z*b2) + (y*b1 +
w*b3)`), `XMMatrixInverse` and the SSE2 `XMVector4Dot` (`(p1 + p3) + (p0 + p2)`) with the same
shuffles in the same order. Rust does not contract or reassociate float operations, and both run
on the calling thread under the same MXCSR, so results, determinants and refusal boundaries are
expected to match bit for bit. Scalar expressions keep their C++ types and order: f32 for camera
and raster values, f64 for the depth bound, `std::min` as `b < a ? b : a`, and the C++ fov bound
`3.14159265f`, which rounds to the same single as `f32::consts::PI`.

Other preserved details: a failing store leaves its destination untouched; refusals leave fields
written before the failing step, as the C++ did; raster history compares the raw bytes of both
rasters, so `-0.0` and `0.0` differ; the crop extent check wraps its `int32` difference and
`int32_t(width)` instead of panicking. `vertical_fov_radians` uses `f32::atan`, which resolves to
the CRT `atanf` the C++ `std::atan(float)` called.

## Checks and open gaps

Checks performed: line-by-line comparison of `camera.cpp` at `9260a14` against the Rust, and of
`xm.rs` against `DirectXMathMatrix.inl` (`XMMatrixMultiply` from line 232, the SSE
`XMMatrixInverse` at lines 885 to 1003, `XMMatrixTranslation` from line 1258) and
`DirectXMathVector.inl` (lines 13392 to 13399); layout assertions on both sides; `cargo fmt`,
`cargo metadata --locked --offline` and `git diff --check`. Formatting used the installed Rust
1.100.0 rustfmt, because the pinned 1.94.1 toolchain and rustup are not installed here; the
crate's existing files were unchanged by it.

Open: build, link, Clippy, `fallout4_camera_contract` and the new Rust unit tests are not run.
Bit-level parity rests on source review, not a differential run; comparing the old `camera.cpp`
against the exports over random and boundary inputs would settle it. A mingw cross build would
link mingw's `atanf` on both sides, which is consistent but not the MSVC result.
