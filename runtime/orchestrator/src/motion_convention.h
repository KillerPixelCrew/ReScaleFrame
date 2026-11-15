// SPDX-License-Identifier: GPL-3.0-only
#pragma once
namespace rsf {
// Canonical input is current-minus-previous UV. DLSS consumes displacement
// from the current pixel toward its previous location. Dense resolve already
// emits that displacement; legacy unspecified encodings retain their scale.
constexpr float dlss_motion_scale(float scale, bool canonical_uv, bool resolved) {
    return canonical_uv && !resolved ? -scale : scale;
}
}
