// SPDX-License-Identifier: MIT
#pragma once
// The D3D11 fixtures compare the HLSL pass with the independent Rust CPU oracle.
namespace rsf::anomaly::sr {
extern "C" {
double rsf_anomaly_tonemap_forward(double) noexcept;
double rsf_anomaly_tonemap_inverse(double) noexcept;
int rsf_anomaly_tonemap_unorm8(double) noexcept;
}
inline double tonemap_forward(double x) noexcept { return rsf_anomaly_tonemap_forward(x); }
inline double tonemap_inverse(double y) noexcept { return rsf_anomaly_tonemap_inverse(y); }
inline int unorm8(double y) noexcept { return rsf_anomaly_tonemap_unorm8(y); }
}
