// SPDX-License-Identifier: MIT
// Retain the independent C++ NGX parameter declaration and catch foreign exceptions.
#include "vendor_census_ngx_params.h"

extern "C" bool rsf_rdr2_fixture_ngx_flags(void* parameter_pointer) noexcept try
{
    auto* parameters = static_cast<NgxParam*>(parameter_pointer);
    parameters->Set("DLSS.Feature.Create.Flags", 1 | 2 | 8);
    return true;
}
catch (...) { return false; }

extern "C" bool rsf_rdr2_fixture_ngx_inputs(
    void* parameter_pointer, void* color, void* depth, void* motion, void* output) noexcept try
{
    auto* parameters = static_cast<NgxParam*>(parameter_pointer);
    parameters->Set("Color", static_cast<ID3D12Resource*>(color));
    parameters->Set("Depth", static_cast<ID3D12Resource*>(depth));
    parameters->Set("MotionVectors", static_cast<ID3D12Resource*>(motion));
    parameters->Set("Output", static_cast<ID3D12Resource*>(output));
    parameters->Set("Jitter.Offset.X", 0.25f);
    parameters->Set("Jitter.Offset.Y", -0.3888889f);
    parameters->Set("MV.Scale.X", 1.f);
    parameters->Set("MV.Scale.Y", 1.f);
    parameters->Set("DLSS.Render.Subrect.Dimensions.Width", 32u);
    parameters->Set("DLSS.Render.Subrect.Dimensions.Height", 24u);
    parameters->Set("Reset", 0);
    return true;
}
catch (...) { return false; }
