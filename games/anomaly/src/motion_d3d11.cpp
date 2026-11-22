// SPDX-License-Identifier: MIT
#include "motion_d3d11.h"
namespace rsf::anomaly::motion {
MotionDevice::~MotionDevice() { rsf_anomaly_motion_device_drop(owner_); }

bool MotionDevice::create(ID3D11Device* device, LogFn log, void* user) noexcept
{
    if (!owner_) owner_ = rsf_anomaly_motion_device_new(rsf_anomaly_motion_device_platform_get());
    return rsf_anomaly_motion_device_create(owner_, device, log, user) != 0;
}

void MotionDevice::reset() noexcept { rsf_anomaly_motion_device_reset(owner_); }

TargetResult MotionDevice::ensure_target(const D3D11_TEXTURE2D_DESC& rt0) noexcept
{
    return static_cast<TargetResult>(rsf_anomaly_motion_device_target(owner_, rt0.Width, rt0.Height,
        rt0.SampleDesc.Count, rt0.ArraySize));
}

void MotionDevice::clear(ID3D11DeviceContext* context) noexcept
{
    rsf_anomaly_motion_clear_target(context, rtv());
}

bool MotionDevice::set_core_vertex(ID3D11DeviceContext* context, const CoreVertexBytes& bytes) noexcept
{
    return rsf_anomaly_motion_device_upload(owner_, context, RSF_MOTION_CORE, bytes.data(), bytes.size()) != 0;
}

bool MotionDevice::set_foliage_vertex(ID3D11DeviceContext* context, const FoliageVertexBytes& bytes) noexcept
{
    return rsf_anomaly_motion_device_upload(owner_, context, RSF_MOTION_FOLIAGE, bytes.data(), bytes.size()) != 0;
}

bool MotionDevice::set_pixel(ID3D11DeviceContext* context, const PixelBytes& bytes) noexcept
{
    return rsf_anomaly_motion_device_upload(owner_, context, RSF_MOTION_PIXEL, bytes.data(), bytes.size()) != 0;
}

bool MotionDevice::set_tess_domain(ID3D11DeviceContext* context, const TessDomainBytes& bytes) noexcept
{
    return rsf_anomaly_motion_device_upload(owner_, context, RSF_MOTION_TESS, bytes.data(), bytes.size()) != 0;
}

}
